// Search query generation.

fn clean_generated_search_query(content: &str) -> String {
    let mut cleaned = content.trim().to_string();
    if cleaned.starts_with("```") {
        cleaned = cleaned
            .trim_start_matches("```text")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string();
    }

    let first_line = cleaned
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let mut query = first_line.trim().to_string();
    for prefix in [
        "搜索查询：",
        "搜索查询:",
        "查询：",
        "查询:",
        "Search query:",
        "Query:",
    ] {
        if query.to_lowercase().starts_with(&prefix.to_lowercase()) {
            query = query[prefix.len()..].trim().to_string();
            break;
        }
    }
    query
        .trim_matches(|c| matches!(c, '"' | '\'' | '“' | '”' | '「' | '」' | '`'))
        .trim()
        .to_string()
}

fn clean_generated_search_query_response(response: &ChatResponse) -> Result<String, String> {
    let query = clean_generated_search_query(&response.content);
    if query.is_empty() {
        let thinking_state = if response
            .thinking
            .as_deref()
            .is_some_and(|thinking| !thinking.trim().is_empty())
        {
            "thinking present"
        } else {
            "thinking absent"
        };
        return Err(format!(
            "empty content ({thinking_state}, content_chars={}, completion_tokens={}, total_tokens={})",
            response.content.chars().count(),
            response.usage.completion_tokens,
            response.usage.total_tokens,
        ));
    }
    if aqbot_core::inline_media::contains_inline_image_data(&query) {
        return Err("generated search query contains inline image data".to_string());
    }
    Ok(truncate_chars(&query, SEARCH_QUERY_CURRENT_CHAR_LIMIT))
}

fn build_search_query_generation_messages_for_attempt(
    history_messages: &[ChatMessage],
    current_content: &str,
    retry: bool,
) -> Vec<ChatMessage> {
    let history = history_messages
        .iter()
        .rev()
        .take(SEARCH_QUERY_HISTORY_LIMIT)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| {
            let role = if message.role == "assistant" {
                "Assistant"
            } else {
                "User"
            };
            let text = truncate_chars(
                &chat_content_text(&message.content).replace(char::is_whitespace, " "),
                SEARCH_QUERY_MESSAGE_CHAR_LIMIT,
            );
            format!("{role}: {text}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let current = truncate_chars(
        &current_content.replace(char::is_whitespace, " "),
        SEARCH_QUERY_CURRENT_CHAR_LIMIT,
    );
    let user_prompt = format!(
        "Conversation history:\n{}\n\nLatest user message:\n{}\n\n{}",
        if history.trim().is_empty() {
            "(none)"
        } else {
            history.as_str()
        },
        current,
        if retry {
            "You must return exactly one non-empty search query. If uncertain, copy the latest user message and resolve missing product names, people, versions, platforms, and subjects from the conversation history."
        } else {
            "Return only the search query."
        },
    );

    vec![
        ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text(
                if retry {
                    "You generate web search queries. The previous attempt returned empty visible content. You must immediately return one concise non-empty plain search-engine query. Do not explain, do not use markdown, do not return labels, and do not leave the answer blank."
                } else {
                    "You generate web search queries. Rewrite the latest user message into one concise search-engine query using the conversation history. Resolve pronouns and follow-up requests from history. If the latest message only grants permission, says to continue, or says you may search/open pages, use the previous unresolved user search intent. Keep important product names, versions, platforms, error text, and proper nouns. Return only the query, with no explanation, quotes, markdown, or labels."
                }
                    .to_string(),
            ),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(user_prompt),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
    ]
}

fn build_search_query_generation_messages(
    history_messages: &[ChatMessage],
    current_content: &str,
) -> Vec<ChatMessage> {
    build_search_query_generation_messages_for_attempt(history_messages, current_content, false)
}

fn build_retry_search_query_generation_messages(
    history_messages: &[ChatMessage],
    current_content: &str,
) -> Vec<ChatMessage> {
    build_search_query_generation_messages_for_attempt(history_messages, current_content, true)
}

fn apply_no_system_role(messages: &mut [ChatMessage], no_system_role: bool) {
    if !no_system_role {
        return;
    }
    for message in messages {
        if message.role == "system" {
            message.role = "user".to_string();
        }
    }
}

fn search_query_prompt_char_count(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|message| chat_content_text(&message.content).chars().count())
        .sum()
}

fn build_search_query_request(
    model_id: &str,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    use_max_completion_tokens: Option<bool>,
) -> ChatRequest {
    ChatRequest {
        model: model_id.to_string(),
        messages,
        stream: false,
        temperature: Some(0.0),
        top_p: None,
        max_tokens: Some(max_tokens),
        tools: None,
        thinking_budget: Some(0),
        thinking_level: Some("off".to_string()),
        reasoning_profile: None,
        use_max_completion_tokens,
        thinking_param_style: None,
        extra_body: None,
    }
}

#[tauri::command]
pub async fn generate_search_query(
    state: State<'_, AppState>,
    conversation_id: String,
    content: String,
) -> Result<String, String> {
    let conversation =
        aqbot_core::repo::conversation::get_conversation(&state.sea_db, &conversation_id)
            .await
            .map_err(|e| e.to_string())?;
    let provider =
        aqbot_core::repo::provider::get_provider(&state.sea_db, &conversation.provider_id)
            .await
            .map_err(|e| e.to_string())?;
    let key_row =
        aqbot_core::repo::provider::get_active_key(&state.sea_db, &conversation.provider_id)
            .await
            .map_err(|e| e.to_string())?;
    let decrypted_key = aqbot_core::crypto::decrypt_key(&key_row.key_encrypted, &state.master_key)
        .map_err(|e| e.to_string())?;
    let settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .unwrap_or_default();
    let resolved_model = aqbot_core::repo::provider::get_model(
        &state.sea_db,
        &conversation.provider_id,
        &conversation.model_id,
    )
    .await
    .ok();
    let model_param_overrides = resolved_model.and_then(|model| model.param_overrides);
    let no_system_role = model_param_overrides
        .as_ref()
        .and_then(|params| params.no_system_role)
        .unwrap_or(false);
    let use_max_completion_tokens = model_param_overrides
        .as_ref()
        .and_then(|params| params.use_max_completion_tokens);

    let messages = aqbot_core::repo::message::list_messages(&state.sea_db, &conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let marker_idx = messages.iter().rposition(|message| {
        message.role == MessageRole::System
            && (message.content == "<!-- context-clear -->"
                || message.content == crate::context_manager::COMPRESSION_MARKER)
    });
    let effective_messages = match marker_idx {
        Some(idx) => &messages[idx + 1..],
        None => &messages[..],
    };
    let file_store = aqbot_core::file_store::FileStore::new();
    let mut history_messages = Vec::new();
    for message in effective_messages {
        if !matches!(message.role, MessageRole::User | MessageRole::Assistant) {
            continue;
        }
        if message.status == "error" || message.status == "partial" {
            continue;
        }
        history_messages.push(
            chat_message_from_message(&file_store, message, false, None, false)
                .map_err(|e| e.to_string())?,
        );
    }

    let current_content = strip_search_enrichment(&content);
    let mut prompt_messages =
        build_search_query_generation_messages(&history_messages, &current_content);
    apply_no_system_role(&mut prompt_messages, no_system_role);

    let ctx = ProviderRequestContext {
        api_key: decrypted_key,
        key_id: key_row.id.clone(),
        provider_id: provider.id.clone(),
        base_url: Some(resolve_base_url_for_type(
            &provider.api_host,
            &provider.provider_type,
        )),
        api_path: provider.api_path.clone(),
        aws_region: provider.aws_region.clone(),
        proxy_config: ProviderProxyConfig::resolve(&provider.proxy_config, &settings),
        custom_headers: provider
            .custom_headers
            .as_ref()
            .and_then(|headers| serde_json::from_str(headers).ok()),
    };
    let registry = ProviderRegistry::create_default();
    let registry_key = provider_type_to_registry_key(&provider.provider_type);
    let adapter = registry
        .get(registry_key)
        .ok_or_else(|| format!("Adapter not found for provider type: {}", registry_key))?;
    let prompt_chars = search_query_prompt_char_count(&prompt_messages);
    let request = build_search_query_request(
        &conversation.model_id,
        prompt_messages,
        SEARCH_QUERY_MAX_TOKENS,
        use_max_completion_tokens,
    );
    let response = adapter
        .chat(&ctx, request)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(
        "[search-query-gen] attempt=initial provider={} model={} prompt_chars={} content_chars={} thinking_present={} completion_tokens={} total_tokens={}",
        provider.id,
        conversation.model_id,
        prompt_chars,
        response.content.chars().count(),
        response.thinking.as_deref().is_some_and(|thinking| !thinking.trim().is_empty()),
        response.usage.completion_tokens,
        response.usage.total_tokens,
    );
    match clean_generated_search_query_response(&response) {
        Ok(query) => return Ok(query),
        Err(first_reason) => {
            tracing::warn!(
                "[search-query-gen] attempt=initial empty provider={} model={} reason={}",
                provider.id,
                conversation.model_id,
                first_reason
            );

            let mut retry_messages =
                build_retry_search_query_generation_messages(&history_messages, &current_content);
            apply_no_system_role(&mut retry_messages, no_system_role);
            let retry_prompt_chars = search_query_prompt_char_count(&retry_messages);
            let retry_request = build_search_query_request(
                &conversation.model_id,
                retry_messages,
                SEARCH_QUERY_RETRY_MAX_TOKENS,
                use_max_completion_tokens,
            );
            let retry_response = adapter
                .chat(&ctx, retry_request)
                .await
                .map_err(|e| e.to_string())?;
            tracing::info!(
                "[search-query-gen] attempt=retry provider={} model={} prompt_chars={} content_chars={} thinking_present={} completion_tokens={} total_tokens={}",
                provider.id,
                conversation.model_id,
                retry_prompt_chars,
                retry_response.content.chars().count(),
                retry_response.thinking.as_deref().is_some_and(|thinking| !thinking.trim().is_empty()),
                retry_response.usage.completion_tokens,
                retry_response.usage.total_tokens,
            );

            match clean_generated_search_query_response(&retry_response) {
                Ok(query) => Ok(query),
                Err(retry_reason) => Err(format!(
                    "AI returned empty search query after retry: initial {first_reason}; retry {retry_reason}"
                )),
            }
        }
    }
}
