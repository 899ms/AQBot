//! Context policy and conversation-history compression.

use aqbot_core::token_counter;
use aqbot_core::types::{ChatContent, ChatMessage, ContextStrategy};
use std::collections::HashSet;

/// Content string for the compression marker message.
pub const COMPRESSION_MARKER: &str = "<!-- context-compressed -->";

/// Default number of trailing compressible messages to leave out of compression.
pub const DEFAULT_COMPRESSION_KEEP_LAST_N: u32 = 3;

/// Content string for the explicit context-clear marker message.
pub const CONTEXT_CLEAR_MARKER: &str = "<!-- context-clear -->";

/// Why messages were omitted from the provider context.
pub const EXCLUSION_REASON_SMART_SUMMARY: &str = "smart_summary";
pub const EXCLUSION_REASON_INPUT_BUDGET: &str = "input_budget";
pub const EXCLUSION_REASON_INPUT_BUDGET_EXCEEDED: &str = "input_budget_exceeded";

/// Result of applying a context strategy to a conversation history.
#[derive(Debug, Clone)]
pub struct ContextBuildResult {
    /// Final provider messages, including system messages.
    pub messages: Vec<ChatMessage>,
    /// Tokens required by the uncompressed, strategy-eligible raw messages.
    pub raw_tokens: usize,
    /// Tokens in [`Self::messages`].
    pub sent_tokens: usize,
    /// Number of user-visible history messages replaced or trimmed.
    pub excluded_message_count: usize,
    /// Stable machine-readable reason for the exclusion or overflow.
    pub exclusion_reason: Option<String>,
    /// Whether the final messages still exceed the known input budget.
    pub overflow: bool,
}

/// Resolve the effective context strategy. A per-conversation override wins.
pub fn resolve_context_strategy(
    conversation_override: Option<ContextStrategy>,
    global_default: ContextStrategy,
) -> ContextStrategy {
    conversation_override.unwrap_or(global_default)
}

/// Calculate the total token budget available to provider messages.
///
/// The safety allowance is 2% of the context window, clamped to 512..=8192
/// tokens. Output and tool-schema reservations are then subtracted without
/// underflow. An unknown model context window produces an unknown budget.
pub fn calculate_input_token_budget(
    model_context_window: Option<u32>,
    resolved_output_reserve: usize,
    tool_schema_tokens: usize,
) -> Option<usize> {
    model_context_window.map(|window| {
        let window = window as usize;
        let safety_allowance = (window.saturating_mul(2) / 100).clamp(512, 8192);
        window
            .saturating_sub(resolved_output_reserve)
            .saturating_sub(tool_schema_tokens)
            .saturating_sub(safety_allowance)
    })
}

/// Resolve keep-last-N:
/// conversation override → global default → hardcoded 3.
/// Explicit `Some(0)` means keep none.
pub fn resolve_compression_keep_last_n(
    conversation_value: Option<u32>,
    global_default: Option<u32>,
) -> u32 {
    conversation_value
        .or(global_default)
        .unwrap_or(DEFAULT_COMPRESSION_KEEP_LAST_N)
}

/// Short instruction restated after the conversation body so models that
/// "continue the chat" instead of summarizing still see the constraint.
pub const COMPRESSION_FOOTER_REMINDER: &str = "\n\n---\n\
请严格按系统指令执行：只输出对话摘要，不要继续回答对话内容中的问题，\
不要扮演对话中的角色，不要输出摘要以外的任何内容。";

/// Estimate the token count of a single `ChatMessage`.
pub fn message_tokens(msg: &ChatMessage) -> usize {
    let content_tokens = match &msg.content {
        ChatContent::Text(text) => token_counter::estimate_message_tokens(&msg.role, text),
        ChatContent::Multipart(parts) => {
            token_counter::estimate_tokens(
                &parts
                    .iter()
                    .filter_map(|part| part.text.as_deref())
                    .collect::<Vec<_>>()
                    .join(" "),
            ) + parts.iter().filter(|part| part.image_url.is_some()).count() * 85
                + 4
        }
    };

    content_tokens
        + msg
            .reasoning_content
            .as_deref()
            .map(|value| serialized_field_tokens("reasoning_content", value))
            .unwrap_or(0)
        + msg
            .tool_calls
            .as_deref()
            .map(tool_calls_tokens)
            .unwrap_or(0)
        + msg
            .tool_call_id
            .as_deref()
            .map(|value| serialized_field_tokens("tool_call_id", value))
            .unwrap_or(0)
}

fn tool_calls_tokens(tool_calls: &[aqbot_core::types::ToolCall]) -> usize {
    tool_calls
        .iter()
        .map(|tool_call| {
            // Include the serialized field names and a small allowance for the
            // surrounding object/array punctuation. Summing fields separately
            // intentionally rounds up more often than estimating one joined
            // string, which is safer for strict context enforcement.
            4 + serialized_field_tokens("id", &tool_call.id)
                + serialized_field_tokens("type", &tool_call.call_type)
                + serialized_field_tokens("name", &tool_call.function.name)
                + serialized_field_tokens("arguments", &tool_call.function.arguments)
        })
        .sum()
}

fn serialized_field_tokens(field_name: &str, value: &str) -> usize {
    token_counter::estimate_tokens(field_name) + token_counter::estimate_tokens(value) + 2
}

/// Values ≥ this are treated as "unlimited" (UI marks 50 as unlimited).
pub const CONTEXT_MESSAGE_LIMIT_UNLIMITED: u32 = 50;

/// Resolve the effective per-message history cap.
///
/// - Conversation override wins over the global default.
/// - `None` at both levels means unlimited (legacy behaviour).
/// - Values ≥ [`CONTEXT_MESSAGE_LIMIT_UNLIMITED`] mean unlimited.
/// - `Some(0)` means "current turn only" and is applied as keep-last-1.
pub fn resolve_message_count_limit(
    conversation_limit: Option<u32>,
    global_default: Option<u32>,
) -> Option<u32> {
    let limit = conversation_limit.or(global_default)?;
    if limit >= CONTEXT_MESSAGE_LIMIT_UNLIMITED {
        None
    } else {
        Some(limit)
    }
}

/// Keep only the most recent `limit` provider history messages.
///
/// `None` leaves history unchanged. `Some(0)` keeps the last message group
/// (current user turn). Tool-call groups are kept atomically so the provider
/// never receives an orphan `tool` result without its assistant call.
pub fn apply_message_count_limit(history: &[ChatMessage], limit: Option<u32>) -> Vec<ChatMessage> {
    let Some(raw_limit) = limit else {
        return history.to_vec();
    };
    if history.is_empty() {
        return Vec::new();
    }

    // 0 ⇒ only the current turn (last group, at least one message).
    let keep = (raw_limit as usize).max(1);
    if history.len() <= keep {
        return history.to_vec();
    }

    let mut total_msgs = 0usize;
    let mut start_idx = history.len();
    let mut end_idx = history.len();

    while end_idx > 0 {
        let group_start = message_group_start(history, end_idx - 1);
        let group_len = end_idx - group_start;

        if total_msgs > 0 && total_msgs + group_len > keep {
            break;
        }

        // Always include at least the trailing group, even if it exceeds `keep`
        // (e.g. a multi-message tool call group).
        total_msgs += group_len;
        start_idx = group_start;
        end_idx = group_start;

        if total_msgs >= keep {
            break;
        }
    }

    history[start_idx..].to_vec()
}

/// Build provider context according to the selected strategy.
///
/// `input_budget` is the complete provider-message budget returned by
/// [`calculate_input_token_budget`], so system messages count against it.
pub fn build_context_for_strategy(
    system_messages: &[ChatMessage],
    history_messages: &[ChatMessage],
    existing_summary: Option<&str>,
    strategy: ContextStrategy,
    input_budget: Option<usize>,
) -> Result<ContextBuildResult, String> {
    let raw_history = raw_history_after_last_clear(history_messages);
    let raw_tokens = total_message_tokens(system_messages) + total_message_tokens(&raw_history);

    match strategy {
        ContextStrategy::SmartSummary => build_smart_summary_context(
            system_messages,
            history_messages,
            existing_summary,
            raw_tokens,
            input_budget,
        ),
        ContextStrategy::RawTruncate => Ok(build_raw_truncate_context(
            system_messages,
            &raw_history,
            raw_tokens,
            input_budget,
        )),
        ContextStrategy::RawStrict => {
            build_raw_strict_context(system_messages, raw_history, raw_tokens, input_budget)
        }
    }
}

fn build_smart_summary_context(
    system_messages: &[ChatMessage],
    history_messages: &[ChatMessage],
    existing_summary: Option<&str>,
    raw_tokens: usize,
    input_budget: Option<usize>,
) -> Result<ContextBuildResult, String> {
    let (history, summarized_count, summary_is_active) =
        smart_summary_history(history_messages, existing_summary.is_some());
    let mut messages = system_messages.to_vec();
    if summary_is_active {
        if let Some(summary) = existing_summary {
            messages.push(summary_message(summary));
        }
    }
    messages.extend(history);

    let sent_tokens = total_message_tokens(&messages);
    let overflow = input_budget.is_some_and(|budget| sent_tokens > budget);
    let exclusion_reason = if overflow {
        Some(EXCLUSION_REASON_INPUT_BUDGET_EXCEEDED.to_string())
    } else if summarized_count > 0 {
        Some(EXCLUSION_REASON_SMART_SUMMARY.to_string())
    } else {
        None
    };

    Ok(ContextBuildResult {
        messages,
        raw_tokens,
        sent_tokens,
        excluded_message_count: summarized_count,
        exclusion_reason,
        overflow,
    })
}

fn build_raw_truncate_context(
    system_messages: &[ChatMessage],
    raw_history: &[ChatMessage],
    raw_tokens: usize,
    input_budget: Option<usize>,
) -> ContextBuildResult {
    let history = match input_budget {
        Some(budget) => {
            let available = budget.saturating_sub(total_message_tokens(system_messages));
            sliding_window(raw_history, available)
        }
        None => raw_history.to_vec(),
    };
    let excluded_message_count = raw_history.len().saturating_sub(history.len());
    let mut messages = system_messages.to_vec();
    messages.extend(history);
    let sent_tokens = total_message_tokens(&messages);
    let overflow = input_budget.is_some_and(|budget| sent_tokens > budget);
    let exclusion_reason = if overflow {
        Some(EXCLUSION_REASON_INPUT_BUDGET_EXCEEDED.to_string())
    } else if excluded_message_count > 0 {
        Some(EXCLUSION_REASON_INPUT_BUDGET.to_string())
    } else {
        None
    };

    ContextBuildResult {
        messages,
        raw_tokens,
        sent_tokens,
        excluded_message_count,
        exclusion_reason,
        overflow,
    }
}

fn build_raw_strict_context(
    system_messages: &[ChatMessage],
    raw_history: Vec<ChatMessage>,
    raw_tokens: usize,
    input_budget: Option<usize>,
) -> Result<ContextBuildResult, String> {
    let budget = input_budget.ok_or_else(|| {
        "raw_strict requires model context-window and output-limit metadata before sending"
            .to_string()
    })?;
    if raw_tokens > budget {
        return Err(format!(
            "raw_strict context exceeds input budget: required {raw_tokens} tokens, available {budget}"
        ));
    }

    let mut messages = system_messages.to_vec();
    messages.extend(raw_history);
    Ok(ContextBuildResult {
        messages,
        raw_tokens,
        sent_tokens: raw_tokens,
        excluded_message_count: 0,
        exclusion_reason: None,
        overflow: false,
    })
}

fn total_message_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(message_tokens).sum()
}

fn raw_history_after_last_clear(history: &[ChatMessage]) -> Vec<ChatMessage> {
    let start = history
        .iter()
        .rposition(is_context_clear_marker)
        .map_or(0, |index| index + 1);

    history[start..]
        .iter()
        .filter(|message| !is_context_boundary_marker(message))
        .cloned()
        .collect()
}

/// Select smart-summary history while supporting both full raw histories and
/// legacy callers that already pass only the post-compression messages.
fn smart_summary_history(
    history: &[ChatMessage],
    has_summary: bool,
) -> (Vec<ChatMessage>, usize, bool) {
    let last_clear = history.iter().rposition(is_context_clear_marker);
    let clear_start = last_clear.map_or(0, |index| index + 1);
    let after_clear = &history[clear_start..];
    let compression = after_clear.iter().rposition(is_compression_marker);

    if has_summary {
        if let Some(marker_index) = compression {
            let summarized_count = after_clear[..marker_index]
                .iter()
                .filter(|message| !is_context_boundary_marker(message))
                .count();
            let messages = after_clear[marker_index + 1..]
                .iter()
                .filter(|message| !is_context_boundary_marker(message))
                .cloned()
                .collect();
            return (messages, summarized_count, true);
        }
    }

    let messages = after_clear
        .iter()
        .filter(|message| !is_context_boundary_marker(message))
        .cloned()
        .collect();
    // A clear marker invalidates a summary that has no newer compression marker.
    (messages, 0, has_summary && last_clear.is_none())
}

fn is_context_boundary_marker(message: &ChatMessage) -> bool {
    is_context_clear_marker(message) || is_compression_marker(message)
}

fn is_context_clear_marker(message: &ChatMessage) -> bool {
    is_text_message(message, CONTEXT_CLEAR_MARKER)
}

fn is_compression_marker(message: &ChatMessage) -> bool {
    is_text_message(message, COMPRESSION_MARKER)
}

fn is_text_message(message: &ChatMessage, expected: &str) -> bool {
    message.role == "system"
        && matches!(&message.content, ChatContent::Text(content) if content == expected)
}

fn summary_message(summary_text: &str) -> ChatMessage {
    ChatMessage {
        role: "system".to_string(),
        content: ChatContent::Text(format!(
            "[对话历史摘要 / Conversation History Summary]\n{}",
            summary_text
        )),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

/// Sliding window: keep as many recent messages as fit within `budget` tokens.
/// Always includes at least the last message to prevent the current user input
/// from being silently dropped.
fn sliding_window(history: &[ChatMessage], budget: usize) -> Vec<ChatMessage> {
    if history.is_empty() {
        return Vec::new();
    }

    let mut total = 0usize;
    let mut start_idx = history.len();
    let mut end_idx = history.len();

    while end_idx > 0 {
        let group_start = message_group_start(history, end_idx - 1);
        let group_tokens: usize = history[group_start..end_idx]
            .iter()
            .map(message_tokens)
            .sum();
        if total + group_tokens > budget {
            break;
        }
        total += group_tokens;
        start_idx = group_start;
        end_idx = group_start;
    }

    // Always include at least the last message
    if start_idx == history.len() {
        start_idx = message_group_start(history, history.len() - 1);
    }

    history[start_idx..].to_vec()
}

fn message_group_start(history: &[ChatMessage], index: usize) -> usize {
    let message = &history[index];
    if message.role != "tool" {
        return index;
    }

    let Some(tool_call_id) = message.tool_call_id.as_deref() else {
        return index;
    };
    if tool_call_id.trim().is_empty() {
        return index;
    }

    for candidate_index in (0..index).rev() {
        let candidate = &history[candidate_index];
        if candidate.role != "assistant" {
            continue;
        }
        let tool_call_ids = candidate
            .tool_calls
            .as_ref()
            .map(|tool_calls| {
                tool_calls
                    .iter()
                    .map(|tool_call| tool_call.id.as_str())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        if tool_call_ids.contains(tool_call_id) {
            return candidate_index;
        }
    }

    index
}

/// Messages that need to be summarized (passed to LLM).
pub struct SummarizationRequest {
    /// Existing summary to merge with, if any.
    pub existing_summary: Option<String>,
    /// Messages to incorporate into the summary.
    pub messages_to_compress: Vec<ChatMessage>,
}

/// Split summary input into non-empty batches that fit `token_budget`.
///
/// Messages are packed greedily in their original order. A single oversized
/// text or multipart-text message is first split on Unicode character
/// boundaries into same-role text messages. The caller must provide a budget
/// large enough for the role and one content character; an impossible budget
/// fails explicitly instead of dropping source text.
pub fn chunk_messages_for_summary(
    messages: &[ChatMessage],
    token_budget: usize,
) -> Result<Vec<Vec<ChatMessage>>, String> {
    if token_budget == 0 {
        return Err("summary token budget must be greater than zero".to_string());
    }

    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut batch_tokens = 0usize;

    for message in messages {
        for piece in split_summary_message(message, token_budget)? {
            let piece_tokens = message_tokens(&piece);
            if piece_tokens > token_budget {
                return Err("summary message chunk exceeds token budget".to_string());
            }
            if !batch.is_empty() && batch_tokens.saturating_add(piece_tokens) > token_budget {
                batches.push(std::mem::take(&mut batch));
                batch_tokens = 0;
            }
            batch_tokens += piece_tokens;
            batch.push(piece);
        }
    }

    if !batch.is_empty() {
        batches.push(batch);
    }
    Ok(batches)
}

fn split_summary_message(
    message: &ChatMessage,
    token_budget: usize,
) -> Result<Vec<ChatMessage>, String> {
    if message_tokens(message) <= token_budget {
        return Ok(vec![message.clone()]);
    }

    let text = message_text_for_summary(message);
    if text.is_empty() {
        let normalized = message_with_summary_text(message, String::new());
        if token_counter::estimate_message_tokens(&message.role, "") > token_budget {
            return Err("summary token budget is too small for message role overhead".to_string());
        }
        return Ok(vec![normalized]);
    }

    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut start_char = 0usize;

    while start_char + 1 < boundaries.len() {
        let end_char =
            largest_fitting_char_end(&text, &boundaries, start_char, &message.role, token_budget)
                .ok_or_else(|| {
                format!(
                    "summary token budget is too small for one character with role {}",
                    message.role
                )
            })?;
        let piece = text[boundaries[start_char]..boundaries[end_char]].to_string();
        pieces.push(message_with_summary_text(message, piece));
        start_char = end_char;
    }

    Ok(pieces)
}

fn largest_fitting_char_end(
    text: &str,
    boundaries: &[usize],
    start_char: usize,
    role: &str,
    token_budget: usize,
) -> Option<usize> {
    let mut low = start_char + 1;
    let mut high = boundaries.len() - 1;
    let mut best_end = None;
    while low <= high {
        let middle = low + (high - low) / 2;
        let candidate = &text[boundaries[start_char]..boundaries[middle]];
        if token_counter::estimate_message_tokens(role, candidate) <= token_budget {
            best_end = Some(middle);
            low = middle + 1;
        } else {
            high = middle.saturating_sub(1);
        }
    }
    best_end
}

fn message_with_summary_text(message: &ChatMessage, text: String) -> ChatMessage {
    let mut piece = message.clone();
    piece.content = ChatContent::Text(text);
    piece
}

fn message_text_for_summary(message: &ChatMessage) -> String {
    match &message.content {
        ChatContent::Text(text) => text.clone(),
        ChatContent::Multipart(parts) => parts
            .iter()
            .filter_map(|part| part.text.as_deref())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Format the conversation body used as compression input (and stored as `source_text`).
pub fn format_compression_source_text(request: &SummarizationRequest) -> String {
    let conversation_text: Vec<String> = request
        .messages_to_compress
        .iter()
        .map(format_message_for_summary)
        .collect();

    let mut parts = Vec::new();
    if let Some(ref summary) = request.existing_summary {
        parts.push(format!("已有摘要：\n{}", summary));
    }
    parts.push(format!(
        "{}对话内容：\n{}",
        if request.existing_summary.is_some() {
            "新增"
        } else {
            ""
        },
        conversation_text.join("\n")
    ));
    parts.join("\n\n")
}

fn format_message_for_summary(m: &ChatMessage) -> String {
    let content_text = message_text_for_summary(m);
    format!("{}: {}", m.role, content_text)
}

pub(crate) fn default_compression_instruction(has_existing_summary: bool) -> &'static str {
    if has_existing_summary {
        "你是一个对话摘要助手。请将以下新增对话内容合并到已有摘要中。\n\n\
         要求：\n\
         1. 保留所有用户明确表达的需求、偏好和决策\n\
         2. 保留关键技术细节（代码片段、配置、错误信息等）\n\
         3. 保留待办事项和未解决的问题\n\
         4. 用简洁的要点形式组织\n\
         5. 如果有冲突信息，以最新的为准\n\
         6. 在输出上限内尽可能完整保留关键事实与原文细节"
    } else {
        "你是一个对话摘要助手。请将以下对话历史压缩为简洁摘要。\n\n\
         要求：\n\
         1. 保留所有用户明确表达的需求、偏好和决策\n\
         2. 保留关键技术细节（代码片段、配置、错误信息等）\n\
         3. 保留待办事项和未解决的问题\n\
         4. 用简洁的要点形式组织\n\
         5. 在输出上限内尽可能完整保留关键事实与原文细节"
    }
}

/// Build the LLM prompt for generating a conversation summary.
pub fn build_summary_prompt(request: &SummarizationRequest) -> Vec<ChatMessage> {
    build_summary_prompt_with_system(
        request,
        default_compression_instruction(request.existing_summary.is_some()),
    )
}

/// Build summary prompt with a custom system instruction (from settings).
pub fn build_summary_prompt_with_custom(
    request: &SummarizationRequest,
    custom_prompt: &str,
) -> Vec<ChatMessage> {
    build_summary_prompt_with_system(request, custom_prompt)
}

fn build_summary_prompt_with_system(
    request: &SummarizationRequest,
    system_prompt: &str,
) -> Vec<ChatMessage> {
    let source = format_compression_source_text(request);
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text(system_prompt.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(format!("{}{}", source, COMPRESSION_FOOTER_REMINDER)),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
    ]
}

/// Split provider history into (to_compress, retained) keeping the last
/// `keep_last_n` messages (group-aware via [`message_group_start`]).
///
/// When `current_user_index` is set (auto path), the current user message and
/// everything after it is always retained, even if `keep_last_n` is 0.
#[cfg(test)]
pub fn split_history_keep_last(
    history_messages: &[ChatMessage],
    keep_last_n: u32,
    current_user_index: Option<usize>,
) -> (Vec<ChatMessage>, Vec<ChatMessage>) {
    if history_messages.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let from_current = current_user_index
        .filter(|&idx| idx < history_messages.len())
        .map(|idx| history_messages.len() - idx)
        .unwrap_or(0);
    let retain_target = (keep_last_n as usize).max(from_current);

    if retain_target == 0 {
        return (history_messages.to_vec(), Vec::new());
    }
    if retain_target >= history_messages.len() {
        return (Vec::new(), history_messages.to_vec());
    }

    // Walk groups from the end until we have at least retain_target messages.
    let mut total_msgs = 0usize;
    let mut start_idx = history_messages.len();
    let mut end_idx = history_messages.len();

    while end_idx > 0 {
        let group_start = message_group_start(history_messages, end_idx - 1);
        let group_len = end_idx - group_start;
        if total_msgs > 0 && total_msgs + group_len > retain_target {
            break;
        }
        total_msgs += group_len;
        start_idx = group_start;
        end_idx = group_start;
        if total_msgs >= retain_target {
            break;
        }
    }

    (
        history_messages[..start_idx].to_vec(),
        history_messages[start_idx..].to_vec(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aqbot_core::types::{ContentPart, ToolCall, ToolCallFunction};

    fn text_message(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: ChatContent::Text(content.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn sliding_window_does_not_keep_orphan_tool_result_without_assistant_call() {
        let mut assistant = text_message("assistant", "");
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        }]);
        assistant.reasoning_content = Some("need file".into());

        let mut tool = text_message("tool", "small tool result");
        tool.tool_call_id = Some("call-1".into());

        let history = vec![
            text_message("user", &"old ".repeat(500)),
            assistant,
            tool,
            text_message("user", "next"),
        ];
        let tool_tokens = message_tokens(&history[2]);
        let current_user_tokens = message_tokens(&history[3]);
        let budget = tool_tokens + current_user_tokens + 1;

        let trimmed = sliding_window(&history, budget);

        assert_eq!(trimmed.len(), 1);
        assert_eq!(trimmed[0].role, "user");
    }

    #[test]
    fn resolve_compression_keep_last_n_defaults_to_three() {
        assert_eq!(resolve_compression_keep_last_n(None, None), 3);
        assert_eq!(resolve_compression_keep_last_n(None, Some(5)), 5);
        assert_eq!(resolve_compression_keep_last_n(Some(0), Some(5)), 0);
        assert_eq!(resolve_compression_keep_last_n(Some(2), Some(5)), 2);
    }

    #[test]
    fn split_history_keep_last_retains_trailing_messages() {
        let history = vec![
            text_message("user", "u1"),
            text_message("assistant", "a1"),
            text_message("user", "u2"),
            text_message("assistant", "a2"),
            text_message("user", "u3"),
        ];

        let (to_compress, retained) = split_history_keep_last(&history, 3, None);
        assert_eq!(to_compress.len(), 2);
        assert_eq!(retained.len(), 3);
        match &retained[0].content {
            ChatContent::Text(s) => assert_eq!(s, "u2"),
            _ => panic!("expected text"),
        }

        let (all, none) = split_history_keep_last(&history, 0, None);
        assert_eq!(all.len(), 5);
        assert!(none.is_empty());

        // Auto path: keep_last_n=0 still retains current user at index 4
        let (compressed, post) = split_history_keep_last(&history, 0, Some(4));
        assert_eq!(compressed.len(), 4);
        assert_eq!(post.len(), 1);
    }

    #[test]
    fn build_summary_prompt_appends_footer_reminder() {
        let request = SummarizationRequest {
            existing_summary: None,
            messages_to_compress: vec![text_message("user", "hello")],
        };
        let messages = build_summary_prompt(&request);
        assert_eq!(messages.len(), 2);
        match &messages[0].content {
            ChatContent::Text(s) => {
                assert!(s.contains("在输出上限内尽可能完整保留关键事实与原文细节"));
                assert!(!s.contains("500 字"));
            }
            _ => panic!("expected text"),
        }
        match &messages[1].content {
            ChatContent::Text(s) => {
                assert!(s.contains("hello"));
                assert!(s.contains(COMPRESSION_FOOTER_REMINDER.trim()));
            }
            _ => panic!("expected text"),
        }

        let source = format_compression_source_text(&request);
        assert!(source.contains("对话内容"));
        assert!(source.contains("hello"));
        assert!(!source.contains(COMPRESSION_FOOTER_REMINDER.trim()));
    }

    #[test]
    fn resolve_message_count_limit_prefers_conversation_over_global() {
        assert_eq!(resolve_message_count_limit(Some(1), Some(10)), Some(1));
        assert_eq!(resolve_message_count_limit(None, Some(3)), Some(3));
        assert_eq!(resolve_message_count_limit(None, None), None);
        assert_eq!(resolve_message_count_limit(Some(50), Some(3)), None);
        assert_eq!(resolve_message_count_limit(None, Some(50)), None);
        assert_eq!(resolve_message_count_limit(Some(0), None), Some(0));
    }

    #[test]
    fn apply_message_count_limit_keeps_last_n_messages() {
        let history = vec![
            text_message("user", "u1"),
            text_message("assistant", "a1"),
            text_message("user", "u2"),
            text_message("assistant", "a2"),
            text_message("user", "u3"),
        ];

        assert_eq!(apply_message_count_limit(&history, None).len(), 5);
        assert_eq!(apply_message_count_limit(&history, Some(50)).len(), 5);

        let limited_one = apply_message_count_limit(&history, Some(1));
        assert_eq!(limited_one.len(), 1);
        assert_eq!(limited_one[0].role, "user");
        match &limited_one[0].content {
            ChatContent::Text(s) => assert_eq!(s, "u3"),
            _ => panic!("expected text"),
        }

        let limited_zero = apply_message_count_limit(&history, Some(0));
        assert_eq!(limited_zero.len(), 1);
        match &limited_zero[0].content {
            ChatContent::Text(s) => assert_eq!(s, "u3"),
            _ => panic!("expected text"),
        }

        let limited_two = apply_message_count_limit(&history, Some(2));
        assert_eq!(limited_two.len(), 2);
        match &limited_two[0].content {
            ChatContent::Text(s) => assert_eq!(s, "a2"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn apply_message_count_limit_keeps_tool_groups_atomic() {
        let mut assistant = text_message("assistant", "");
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        }]);

        let mut tool = text_message("tool", "file contents");
        tool.tool_call_id = Some("call-1".into());

        let history = vec![
            text_message("user", "old"),
            assistant,
            tool,
            text_message("user", "next"),
        ];

        // keep=1 → only current user
        let only_current = apply_message_count_limit(&history, Some(1));
        assert_eq!(only_current.len(), 1);
        assert_eq!(only_current[0].role, "user");

        // keep=2 would try to take user + one prior, but tool group is 2 msgs;
        // taking the tool result alone is invalid, so group stays together.
        // With keep=2 we get current user (1) + cannot add full tool group (2)
        // without exceeding → only current user? Let's check: total starts 0,
        // last group is user (1 msg) → total=1, then next group is tool-only
        // index for tool: message_group_start finds assistant. Group is
        // assistant+tool (2 msgs). total_msgs=1, 1+2=3 > keep=2, break.
        // Result: only current user.
        let keep_two = apply_message_count_limit(&history, Some(2));
        assert_eq!(keep_two.len(), 1);
        assert_eq!(keep_two[0].role, "user");

        // keep=3 → current user (1) + full tool group (2) = 3
        let keep_three = apply_message_count_limit(&history, Some(3));
        assert_eq!(keep_three.len(), 3);
        assert_eq!(keep_three[0].role, "assistant");
        assert_eq!(keep_three[1].role, "tool");
        assert_eq!(keep_three[2].role, "user");
    }

    #[test]
    fn context_strategy_resolves_override_and_keep_last_limit_is_explicit() {
        assert_eq!(
            resolve_context_strategy(None, ContextStrategy::SmartSummary),
            ContextStrategy::SmartSummary
        );
        assert_eq!(
            resolve_context_strategy(
                Some(ContextStrategy::RawStrict),
                ContextStrategy::RawTruncate,
            ),
            ContextStrategy::RawStrict
        );
    }

    #[test]
    fn dynamic_input_budget_reserves_output_tools_and_clamped_safety_allowance() {
        assert_eq!(calculate_input_token_budget(None, 1_000, 500), None);
        // 2% is below the 512-token minimum.
        assert_eq!(
            calculate_input_token_budget(Some(10_000), 1_000, 500),
            Some(7_988)
        );
        // 2% lies inside the clamp range.
        assert_eq!(
            calculate_input_token_budget(Some(100_000), 1_000, 500),
            Some(96_500)
        );
        // 2% exceeds the 8192-token maximum.
        assert_eq!(
            calculate_input_token_budget(Some(1_000_000), 1_000, 500),
            Some(990_308)
        );
        assert_eq!(calculate_input_token_budget(Some(500), 0, 0), Some(0));
    }

    #[test]
    fn compression_source_preserves_long_utf8_content_without_truncation() {
        let content = format!("{}完整结尾🙂", "你好🙂".repeat(800));
        assert!(content.len() > 2000);
        let request = SummarizationRequest {
            existing_summary: None,
            messages_to_compress: vec![text_message("user", &content)],
        };

        let source = format_compression_source_text(&request);

        assert!(source.contains(&content));
        assert!(source.ends_with("完整结尾🙂"));
        assert!(!source.contains("[已截断]"));
    }

    #[test]
    fn summary_chunks_pack_messages_greedily_without_exceeding_budget() {
        let messages = vec![
            text_message("user", "aaaa"),
            text_message("user", "bbbb"),
            text_message("user", "cccc"),
        ];
        let per_message = message_tokens(&messages[0]);

        let chunks = chunk_messages_for_summary(&messages, per_message * 2).unwrap();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1].len(), 1);
        assert!(chunks.iter().all(|chunk| !chunk.is_empty()));
        assert!(chunks
            .iter()
            .all(|chunk| total_message_tokens(chunk) <= per_message * 2));
    }

    #[test]
    fn summary_chunks_split_long_json_on_chinese_and_emoji_boundaries() {
        let source = format!(
            "{{\"中文\":\"{}\",\"emoji\":\"{}\"}}",
            "数据".repeat(200),
            "🙂🚀".repeat(200)
        );

        let chunks = chunk_messages_for_summary(&[text_message("user", &source)], 40).unwrap();

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| !chunk.is_empty()));
        assert!(chunks.iter().all(|chunk| total_message_tokens(chunk) <= 40));
        assert!(chunks
            .iter()
            .flatten()
            .all(|message| message.role == "user"));
        assert_eq!(concatenate_text(&chunks), source);
    }

    #[test]
    fn summary_chunks_split_multipart_text_without_losing_unicode() {
        let expected = format!("{} {}", "中文".repeat(120), "🙂".repeat(160));
        let message = ChatMessage {
            role: "assistant".to_string(),
            content: ChatContent::Multipart(vec![
                ContentPart {
                    r#type: "text".to_string(),
                    text: Some("中文".repeat(120)),
                    image_url: None,
                },
                ContentPart {
                    r#type: "text".to_string(),
                    text: Some("🙂".repeat(160)),
                    image_url: None,
                },
            ]),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        };

        let chunks = chunk_messages_for_summary(&[message], 32).unwrap();

        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| !chunk.is_empty() && total_message_tokens(chunk) <= 32));
        assert_eq!(concatenate_text(&chunks), expected);
    }

    #[test]
    fn summary_chunks_reject_impossible_budget_without_panicking() {
        let error = chunk_messages_for_summary(&[text_message("user", "🙂")], 1).unwrap_err();

        assert!(error.contains("token budget"));
    }

    #[test]
    fn smart_summary_uses_latest_compression_marker_without_silent_trimming() {
        let system = vec![text_message("system", "system")];
        let history = vec![
            text_message("user", &"old user detail ".repeat(100)),
            text_message("assistant", &"old assistant detail ".repeat(100)),
            text_message("system", COMPRESSION_MARKER),
            text_message("user", "current user"),
        ];

        let result = build_context_for_strategy(
            &system,
            &history,
            Some("preserved summary"),
            ContextStrategy::SmartSummary,
            Some(usize::MAX),
        )
        .unwrap();

        assert_eq!(result.messages.len(), 3);
        assert_eq!(result.excluded_message_count, 2);
        assert_eq!(
            result.exclusion_reason.as_deref(),
            Some(EXCLUSION_REASON_SMART_SUMMARY)
        );
        assert!(result.sent_tokens < result.raw_tokens);
        assert!(message_contains(&result.messages[1], "preserved summary"));
        assert!(message_contains(&result.messages[2], "current user"));

        let overflowing = build_context_for_strategy(
            &system,
            &history,
            Some("preserved summary"),
            ContextStrategy::SmartSummary,
            Some(1),
        )
        .unwrap();
        assert!(overflowing.overflow);
        // The current message remains present even when the budget is exceeded.
        assert!(overflowing
            .messages
            .iter()
            .any(|message| message_contains(message, "current user")));
    }

    #[test]
    fn raw_truncate_ignores_summary_and_keeps_tool_groups_atomic() {
        let system = vec![text_message("system", "system")];
        let mut assistant = text_message("assistant", "calling tool");
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        }]);
        let mut tool = text_message("tool", "tool result");
        tool.tool_call_id = Some("call-1".into());
        let history = vec![
            text_message("user", &"old ".repeat(200)),
            text_message("system", COMPRESSION_MARKER),
            assistant,
            tool,
            text_message("user", "current"),
        ];
        let trailing_budget = total_message_tokens(&system)
            + total_message_tokens(&raw_history_after_last_clear(&history)[1..]);

        let result = build_context_for_strategy(
            &system,
            &history,
            Some("must be ignored"),
            ContextStrategy::RawTruncate,
            Some(trailing_budget),
        )
        .unwrap();

        assert_eq!(result.messages.len(), 4);
        assert_eq!(result.messages[1].role, "assistant");
        assert_eq!(result.messages[2].role, "tool");
        assert_eq!(result.messages[3].role, "user");
        assert_eq!(result.excluded_message_count, 1);
        assert_eq!(
            result.exclusion_reason.as_deref(),
            Some(EXCLUSION_REASON_INPUT_BUDGET)
        );
        assert!(!result
            .messages
            .iter()
            .any(|message| message_contains(message, "must be ignored")));

        let current_only_budget = total_message_tokens(&system)
            + message_tokens(history.last().expect("current message"));
        let current_only = build_context_for_strategy(
            &system,
            &history,
            None,
            ContextStrategy::RawTruncate,
            Some(current_only_budget),
        )
        .unwrap();
        assert_eq!(current_only.messages.len(), 2);
        assert_eq!(current_only.messages[1].role, "user");
    }

    #[test]
    fn raw_modes_restore_precompression_text_but_respect_last_context_clear() {
        let system = vec![text_message("system", "system")];
        let history = vec![
            text_message("user", "before clear"),
            text_message("system", COMPRESSION_MARKER),
            text_message("system", CONTEXT_CLEAR_MARKER),
            text_message("user", "after clear before compression"),
            text_message("system", COMPRESSION_MARKER),
            text_message("assistant", "after compression"),
        ];
        let raw_history = raw_history_after_last_clear(&history);
        let exact_budget = total_message_tokens(&system) + total_message_tokens(&raw_history);

        let result = build_context_for_strategy(
            &system,
            &history,
            Some("must be ignored"),
            ContextStrategy::RawStrict,
            Some(exact_budget),
        )
        .unwrap();

        assert_eq!(result.messages.len(), 3);
        assert!(result
            .messages
            .iter()
            .any(|message| message_contains(message, "after clear before compression")));
        assert!(result
            .messages
            .iter()
            .any(|message| message_contains(message, "after compression")));
        assert!(!result
            .messages
            .iter()
            .any(|message| message_contains(message, "before clear")));
        assert_eq!(result.raw_tokens, exact_budget);
        assert_eq!(result.sent_tokens, exact_budget);
    }

    #[test]
    fn raw_strict_rejects_unknown_or_insufficient_budget() {
        let history = vec![text_message("user", "important raw text")];

        let unknown =
            build_context_for_strategy(&[], &history, None, ContextStrategy::RawStrict, None)
                .unwrap_err();
        assert!(unknown.contains("context-window and output-limit metadata"));

        let required = total_message_tokens(&history);
        let overflow = build_context_for_strategy(
            &[],
            &history,
            None,
            ContextStrategy::RawStrict,
            Some(required - 1),
        )
        .unwrap_err();
        assert!(overflow.contains("exceeds input budget"));
    }

    #[test]
    fn raw_strict_counts_large_tool_payload_metadata_before_sending() {
        let mut assistant = text_message("assistant", "");
        assistant.reasoning_content = Some("需要调用工具并保留推理上下文🙂".repeat(256));
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call-large-json".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "process_payload".into(),
                arguments: format!(r#"{{"payload":"{}"}}"#, "长参数🙂".repeat(2_000)),
            },
        }]);

        let mut tool_result = text_message("tool", &"工具结果🙂".repeat(2_000));
        tool_result.tool_call_id = Some("call-large-json".into());
        let history = vec![assistant.clone(), tool_result.clone()];

        let mut content_only_assistant = assistant;
        content_only_assistant.reasoning_content = None;
        content_only_assistant.tool_calls = None;
        let mut content_only_tool_result = tool_result;
        content_only_tool_result.tool_call_id = None;
        let content_only_budget =
            total_message_tokens(&[content_only_assistant, content_only_tool_result]);

        assert!(total_message_tokens(&history) > content_only_budget);
        let error = build_context_for_strategy(
            &[],
            &history,
            None,
            ContextStrategy::RawStrict,
            Some(content_only_budget),
        )
        .unwrap_err();

        assert!(error.contains("exceeds input budget"));
    }

    #[test]
    fn raw_truncate_reports_overflow_when_current_group_cannot_fit() {
        let history = vec![text_message("user", "current message")];

        let result =
            build_context_for_strategy(&[], &history, None, ContextStrategy::RawTruncate, Some(0))
                .unwrap();

        assert!(result.overflow);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(
            result.exclusion_reason.as_deref(),
            Some(EXCLUSION_REASON_INPUT_BUDGET_EXCEEDED)
        );
    }

    fn message_contains(message: &ChatMessage, expected: &str) -> bool {
        match &message.content {
            ChatContent::Text(content) => content.contains(expected),
            ChatContent::Multipart(parts) => parts
                .iter()
                .filter_map(|part| part.text.as_deref())
                .any(|content| content.contains(expected)),
        }
    }

    fn concatenate_text(chunks: &[Vec<ChatMessage>]) -> String {
        chunks
            .iter()
            .flatten()
            .map(|message| match &message.content {
                ChatContent::Text(content) => content.as_str(),
                ChatContent::Multipart(_) => panic!("oversized messages should normalize to text"),
            })
            .collect()
    }
}
