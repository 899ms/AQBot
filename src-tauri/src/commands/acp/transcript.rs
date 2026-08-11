// ACP tool transcript state and inline marker serialization.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedAcpToolCall {
    tool_call_id: String,
    tool_name: String,
    status: String,
    input: Option<String>,
    output: Option<String>,
    approval_status: Option<String>,
    approval_option_id: Option<String>,
    approval_option_kind: Option<String>,
    approval_label: Option<String>,
    #[serde(skip)]
    sequence: u64,
}
fn next_tool_sequence(
    tools: &HashMap<String, PersistedAcpToolCall>,
    tool_call_id: &str,
    next_sequence: &mut u64,
) -> u64 {
    tools.get(tool_call_id).map_or_else(
        || {
            let sequence = *next_sequence;
            *next_sequence += 1;
            sequence
        },
        |tool| tool.sequence,
    )
}

fn record_tool_call(
    tools: &mut HashMap<String, PersistedAcpToolCall>,
    next_sequence: &mut u64,
    tool_call_id: &str,
    title: &Option<String>,
    kind: &Option<String>,
    status: &Option<String>,
    raw: &serde_json::Value,
) {
    let sequence = next_tool_sequence(tools, tool_call_id, next_sequence);
    let previous = tools.remove(tool_call_id);
    let tool_name = kind
        .clone()
        .or_else(|| title.clone())
        .or_else(|| previous.as_ref().map(|tool| tool.tool_name.clone()))
        .unwrap_or_else(|| "tool".into());
    let status = status
        .clone()
        .or_else(|| previous.as_ref().map(|tool| tool.status.clone()))
        .unwrap_or_else(|| "queued".into());
    let input =
        tool_input_detail(raw).or_else(|| previous.as_ref().and_then(|tool| tool.input.clone()));
    let output =
        tool_output_detail(raw).or_else(|| previous.as_ref().and_then(|tool| tool.output.clone()));
    tools.insert(
        tool_call_id.to_string(),
        PersistedAcpToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool_name,
            status,
            input,
            output,
            approval_status: previous
                .as_ref()
                .and_then(|tool| tool.approval_status.clone()),
            approval_option_id: previous
                .as_ref()
                .and_then(|tool| tool.approval_option_id.clone()),
            approval_option_kind: previous
                .as_ref()
                .and_then(|tool| tool.approval_option_kind.clone()),
            approval_label: previous
                .as_ref()
                .and_then(|tool| tool.approval_label.clone()),
            sequence,
        },
    );
}

fn record_interaction_outcome(
    tools: &mut HashMap<String, PersistedAcpToolCall>,
    next_sequence: &mut u64,
    tool_call_id: &str,
    interaction_kind: AcpInteractionKind,
    outcome: AcpInteractionOutcome,
    option_id: Option<&str>,
    option_kind: Option<&str>,
    option_label: Option<&str>,
) {
    let sequence = next_tool_sequence(tools, tool_call_id, next_sequence);
    let tool = tools
        .entry(tool_call_id.to_string())
        .or_insert_with(|| PersistedAcpToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool_name: "tool".into(),
            status: "queued".into(),
            input: None,
            output: None,
            approval_status: None,
            approval_option_id: None,
            approval_option_kind: None,
            approval_label: None,
            sequence,
        });
    if interaction_kind != AcpInteractionKind::Permission {
        if outcome == AcpInteractionOutcome::Selected && tool.output.is_none() {
            tool.output = option_label
                .filter(|label| !label.is_empty())
                .map(str::to_owned)
                .or_else(|| option_id.map(|id| format!("aqbot:questionnaire:{id}")));
        }
        return;
    }

    let approval_status = match outcome {
        AcpInteractionOutcome::Selected
            if option_kind.is_some_and(|kind| {
                matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "allowonce" | "allow_once" | "allowalways" | "allow_always"
                )
            }) =>
        {
            "approved"
        }
        AcpInteractionOutcome::Selected => "denied",
        AcpInteractionOutcome::Cancelled => "cancelled",
        AcpInteractionOutcome::Expired => "expired",
    };
    tool.approval_status = Some(approval_status.into());
    tool.approval_option_id = option_id.map(str::to_owned);
    tool.approval_option_kind = option_kind.map(str::to_owned);
    tool.approval_label = option_label.map(str::to_owned);
    if approval_status != "approved" {
        tool.status = "cancelled".into();
    }
}

fn finalize_unfinished_tool_calls(
    tools: &mut HashMap<String, PersistedAcpToolCall>,
    terminal_status: &str,
) {
    for tool in tools.values_mut() {
        let status = tool.status.to_ascii_lowercase();
        let terminal = matches!(
            status.as_str(),
            "completed" | "success" | "failed" | "error" | "cancelled" | "canceled"
        );
        if !terminal {
            tool.status = terminal_status.to_string();
        }
    }
}

#[cfg(test)]
mod tool_transcript_tests {
    use super::*;

    #[test]
    fn permission_outcome_survives_a_later_tool_call_event() {
        let mut tools = HashMap::new();
        let mut next_sequence = 0;
        record_interaction_outcome(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            AcpInteractionKind::Permission,
            AcpInteractionOutcome::Selected,
            Some("allow-once"),
            Some("AllowOnce"),
            Some("Allow once"),
        );

        record_tool_call(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            &Some("Run command".into()),
            &Some("execute".into()),
            &Some("running".into()),
            &serde_json::json!({ "rawInput": { "command": "pwd" } }),
        );

        let tool = tools.get("tool-1").expect("merged tool call");
        assert_eq!(tool.approval_status.as_deref(), Some("approved"));
        assert_eq!(tool.approval_option_id.as_deref(), Some("allow-once"));
        assert_eq!(tool.approval_option_kind.as_deref(), Some("AllowOnce"));
        assert_eq!(tool.approval_label.as_deref(), Some("Allow once"));
        assert_eq!(tool.tool_name, "execute");
        assert_eq!(tool.sequence, 0);
        assert_eq!(next_sequence, 1);

        let serialized = serde_json::to_value(tool).expect("serialize persisted tool");
        assert_eq!(serialized["approvalStatus"], "approved");
        assert_eq!(serialized["approvalOptionId"], "allow-once");
        assert_eq!(serialized["approvalOptionKind"], "AllowOnce");
        assert_eq!(serialized["approvalLabel"], "Allow once");
    }

    #[test]
    fn permission_terminal_outcomes_keep_their_meaning() {
        for (outcome, kind, expected) in [
            (AcpInteractionOutcome::Cancelled, None, "cancelled"),
            (AcpInteractionOutcome::Expired, None, "expired"),
            (
                AcpInteractionOutcome::Selected,
                Some("RejectOnce"),
                "denied",
            ),
        ] {
            let mut tools = HashMap::new();
            let mut next_sequence = 0;
            record_interaction_outcome(
                &mut tools,
                &mut next_sequence,
                "tool-1",
                AcpInteractionKind::Permission,
                outcome,
                Some("deny"),
                kind,
                Some("Deny"),
            );
            assert_eq!(tools["tool-1"].approval_status.as_deref(), Some(expected));
            assert_eq!(tools["tool-1"].status, "cancelled");
        }
    }

    #[test]
    fn question_and_plan_outcomes_preserve_answers_until_the_agent_finishes_the_tool() {
        for interaction_kind in [AcpInteractionKind::Question, AcpInteractionKind::PlanReview] {
            let mut tools = HashMap::new();
            let mut next_sequence = 0;
            record_interaction_outcome(
                &mut tools,
                &mut next_sequence,
                "tool-1",
                interaction_kind,
                AcpInteractionOutcome::Selected,
                Some("choice-1"),
                None,
                Some("Use SQLite"),
            );

            assert_eq!(tools["tool-1"].status, "queued");
            assert_eq!(tools["tool-1"].output.as_deref(), Some("Use SQLite"));
            assert_eq!(tools["tool-1"].approval_status, None);
        }
    }

    #[test]
    fn empty_plan_action_persists_its_semantic_result_id() {
        let mut tools = HashMap::new();
        let mut next_sequence = 0;

        record_interaction_outcome(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            AcpInteractionKind::PlanReview,
            AcpInteractionOutcome::Selected,
            Some("skip_interview"),
            None,
            Some(""),
        );

        assert_eq!(
            tools["tool-1"].output.as_deref(),
            Some("aqbot:questionnaire:skip_interview")
        );
    }

    #[test]
    fn canonical_tool_output_wins_if_it_arrives_before_the_interaction_closes() {
        let mut tools = HashMap::from([(
            "tool-1".into(),
            PersistedAcpToolCall {
                tool_call_id: "tool-1".into(),
                tool_name: "ask_user_question".into(),
                status: "success".into(),
                input: None,
                output: Some("Agent-recorded result".into()),
                approval_status: None,
                approval_option_id: None,
                approval_option_kind: None,
                approval_label: None,
                sequence: 0,
            },
        )]);
        let mut next_sequence = 1;

        record_interaction_outcome(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            AcpInteractionKind::PlanReview,
            AcpInteractionOutcome::Selected,
            Some("skip_interview"),
            None,
            Some(""),
        );

        assert_eq!(
            tools["tool-1"].output.as_deref(),
            Some("Agent-recorded result")
        );
    }

    #[test]
    fn turn_terminal_state_closes_only_unfinished_tool_calls() {
        let tool = |id: &str, status: &str| PersistedAcpToolCall {
            tool_call_id: id.into(),
            tool_name: "execute".into(),
            status: status.into(),
            input: None,
            output: None,
            approval_status: None,
            approval_option_id: None,
            approval_option_kind: None,
            approval_label: None,
            sequence: 0,
        };
        let mut tools = HashMap::from([
            ("queued".into(), tool("queued", "queued")),
            ("running".into(), tool("running", "in_progress")),
            ("success".into(), tool("success", "completed")),
            ("failed".into(), tool("failed", "error")),
        ]);

        finalize_unfinished_tool_calls(&mut tools, "cancelled");

        assert_eq!(tools["queued"].status, "cancelled");
        assert_eq!(tools["running"].status, "cancelled");
        assert_eq!(tools["success"].status, "completed");
        assert_eq!(tools["failed"].status, "error");
    }

    #[test]
    fn tool_marker_truncates_unicode_on_character_boundaries() {
        let title = format!("{}🙂🙂", "中".repeat(159));
        let marker = build_acp_tool_call_marker(
            "tool-unicode",
            "assistant-unicode",
            &Some(title.clone()),
            &Some("execute".into()),
            &serde_json::Value::Null,
        );
        let expected = format!("{}…", title.chars().take(160).collect::<String>());

        assert!(marker.contains(&expected));
        assert!(!marker.contains(&title));
        assert!(marker.contains("message=\"assistant-unicode\""));
    }

    #[test]
    fn plan_marker_embeds_request_and_message_ids() {
        let marker = build_acp_plan_marker(
            "plan-1",
            "assistant-1",
            &Some("Plan review".into()),
            "## Plan\n1. Inspect\n2. Ship",
            "pending",
        );
        assert!(marker.contains(
            "<acp-plan data-aqbot=\"1\" id=\"plan-1\" message=\"assistant-1\" status=\"pending\" title=\"Plan review\">"
        ));
        assert!(marker.contains("## Plan"));
        assert!(marker.contains("1. Inspect"));
        assert!(marker.contains("</acp-plan>"));
    }

    #[test]
    fn plan_marker_escapes_body_so_nested_tags_do_not_close_early() {
        let marker = build_acp_plan_marker(
            "plan-2",
            "assistant-2",
            &None,
            "use </acp-plan> carefully & <br>",
            "approved",
        );
        assert!(marker.contains("status=\"approved\""));
        assert!(marker.contains("&lt;/acp-plan&gt;"));
        assert!(marker.contains("&amp;"));
        assert!(marker.contains("&lt;br&gt;"));
        assert!(marker.ends_with("</acp-plan>\n\n") || marker.contains("</acp-plan>\n\n"));
    }

    #[test]
    fn patch_plan_marker_status_rewrites_existing_marker() {
        let mut acc = build_acp_plan_marker(
            "plan-1",
            "assistant-1",
            &Some("Plan".into()),
            "body",
            "pending",
        );
        assert!(patch_acp_plan_marker_status(&mut acc, "plan-1", "approved"));
        assert!(acc.contains("status=\"approved\""));
        assert!(!acc.contains("status=\"pending\""));
    }

    #[test]
    fn plan_review_status_distinguishes_native_cancel_from_requested_changes() {
        assert_eq!(
            plan_review_status_from_outcome(AcpInteractionOutcome::Cancelled, None),
            "abandoned"
        );
        assert_eq!(
            plan_review_status_from_outcome(AcpInteractionOutcome::Selected, Some("revise_plan"),),
            "cancelled"
        );
        assert_eq!(
            plan_review_status_from_outcome(
                AcpInteractionOutcome::Selected,
                Some("implement_plan"),
            ),
            "approved"
        );
    }

    #[test]
    fn extract_plan_content_prefers_plan_content_field() {
        let raw = serde_json::json!({
            "title": "short",
            "planContent": "## Full plan body",
            "description": "fallback"
        });
        assert_eq!(
            extract_plan_content_from_raw(&raw).as_deref(),
            Some("## Full plan body")
        );
    }
}

fn json_detail(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    Some(match value {
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    })
}

fn tool_input_detail(raw: &serde_json::Value) -> Option<String> {
    json_detail(
        raw.get("rawInput")
            .or_else(|| raw.get("raw_input"))
            .or_else(|| raw.get("input"))
            .or_else(|| raw.get("locations")),
    )
}

fn tool_output_detail(raw: &serde_json::Value) -> Option<String> {
    json_detail(
        raw.get("rawOutput")
            .or_else(|| raw.get("raw_output"))
            .or_else(|| raw.get("output"))
            .or_else(|| raw.get("content")),
    )
}

// ---------- Inline tool-call markers (chat-agent parity) ----------

fn xml_attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml_text_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Pull plan-review body from the permission/request payload so the marker
/// can be fully reconstructed after a page reload.
fn extract_plan_content_from_raw(raw: &serde_json::Value) -> Option<String> {
    for key in [
        "planContent",
        "plan_content",
        "content",
        "description",
        "plan",
    ] {
        if let Some(text) = raw.get(key).and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn plan_review_status_from_outcome(
    outcome: AcpInteractionOutcome,
    selected_option_id: Option<&str>,
) -> &'static str {
    let option_status = selected_option_id.and_then(|id| {
        let normalized: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match normalized.as_str() {
            "approved" | "approve" | "implementplan" => Some("approved"),
            "cancelled" | "cancel" | "reviseplan" | "plan" => Some("cancelled"),
            "abandoned" | "abandon" => Some("abandoned"),
            _ => None,
        }
    });
    match outcome {
        AcpInteractionOutcome::Expired => "expired",
        AcpInteractionOutcome::Cancelled => option_status.unwrap_or("abandoned"),
        AcpInteractionOutcome::Selected => option_status.unwrap_or("approved"),
    }
}

/// Rewrite `status="..."` on an existing inline plan marker so reloads keep
/// the final review outcome (approved / cancelled / abandoned / expired).
fn patch_acp_plan_marker_status(acc: &mut String, request_id: &str, status: &str) -> bool {
    let id_attr = format!("id=\"{}\"", xml_attr_escape(request_id));
    let Some(id_pos) = acc.find(&id_attr) else {
        return false;
    };
    // Walk back to the opening `<acp-plan` of this marker.
    let prefix = &acc[..id_pos];
    let Some(tag_rel) = prefix.rfind("<acp-plan") else {
        return false;
    };
    let tag_start = tag_rel;
    let Some(tag_end_rel) = acc[tag_start..].find('>') else {
        return false;
    };
    let tag_end = tag_start + tag_end_rel;
    let open_tag = &acc[tag_start..=tag_end];
    if !open_tag.starts_with("<acp-plan") {
        return false;
    }
    let status_attr = format!("status=\"{}\"", xml_attr_escape(status));
    let new_open = if let Some(status_start) = open_tag.find("status=\"") {
        let after = &open_tag[status_start + "status=\"".len()..];
        let Some(quote_end) = after.find('"') else {
            return false;
        };
        format!(
            "{}{}{}",
            &open_tag[..status_start],
            status_attr,
            &open_tag[status_start + "status=\"".len() + quote_end + 1..]
        )
    } else {
        // Insert status just before the closing `>`.
        format!("{} {}>", &open_tag[..open_tag.len() - 1], status_attr)
    };
    acc.replace_range(tag_start..=tag_end, &new_open);
    true
}

/// Build an inline `<acp-plan>` marker so plan reviews render mid-conversation
/// in chronological order (before any later assistant text in the same turn).
///
/// The **body holds the full plan markdown** so the card can be reconstructed
/// after a page refresh without relying on in-memory store state.
fn build_acp_plan_marker(
    request_id: &str,
    message_id: &str,
    title: &Option<String>,
    content: &str,
    status: &str,
) -> String {
    let label = title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("plan");
    let body = if content.trim().is_empty() {
        label
    } else {
        content
    };
    format!(
        "\n\n<acp-plan data-aqbot=\"1\" id=\"{}\" message=\"{}\" status=\"{}\" title=\"{}\">{}</acp-plan>\n\n",
        xml_attr_escape(request_id),
        xml_attr_escape(message_id),
        xml_attr_escape(status),
        xml_attr_escape(label),
        xml_text_escape(body),
    )
}

/// Build an inline `<tool-call>` marker so tools render mid-conversation
/// in call order (same contract as chat agent mode).
fn build_acp_tool_call_marker(
    tool_call_id: &str,
    message_id: &str,
    title: &Option<String>,
    kind: &Option<String>,
    raw: &serde_json::Value,
) -> String {
    // Prefer short kind as the chip name; fall back to title / "tool"
    let name = kind
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            title
                .as_deref()
                .map(|t| t.split_whitespace().next().unwrap_or(t))
                .filter(|s| !s.is_empty() && s.len() <= 32)
        })
        .unwrap_or("tool");

    let mut summary = title.clone().unwrap_or_default();
    if summary.is_empty() {
        // rawInput.command / path / filePath etc.
        let input = raw
            .get("rawInput")
            .or_else(|| raw.get("raw_input"))
            .or_else(|| raw.get("input"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if let Some(obj) = input.as_object() {
            for key in [
                "command",
                "path",
                "filePath",
                "file_path",
                "pattern",
                "query",
            ] {
                if let Some(v) = obj.get(key).and_then(|x| x.as_str()) {
                    summary = v.to_string();
                    break;
                }
            }
        }
        if summary.is_empty() {
            if let Some(locs) = raw.get("locations").and_then(|v| v.as_array()) {
                if let Some(path) = locs
                    .first()
                    .and_then(|l| l.get("path").or_else(|| l.get("uri")))
                    .and_then(|v| v.as_str())
                {
                    summary = path.to_string();
                }
            }
        }
    }

    // Keep summary readable in the chip
    if summary.chars().count() > 160 {
        summary = format!("{}…", summary.chars().take(160).collect::<String>());
    }
    // Collapse newlines for attr-like chip text
    summary = summary.replace('\n', " ").replace('\r', " ");

    format!(
        "\n\n<tool-call data-aqbot=\"1\" id=\"{}\" message=\"{}\" name=\"{}\">{}</tool-call>\n\n",
        xml_attr_escape(tool_call_id),
        xml_attr_escape(message_id),
        xml_attr_escape(name),
        xml_text_escape(&summary),
    )
}
