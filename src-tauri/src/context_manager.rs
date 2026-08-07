//! Context manager for conversation history compression.
//!
//! Two modes:
//! - **Sliding window** (compression OFF): trims oldest messages to fit the token budget.
//! - **Compression** (manual or auto): all messages are compressed into an LLM summary,
//!   a `<!-- context-compressed -->` marker is inserted, and subsequent sends use
//!   the summary + only messages after the marker.

use aqbot_core::token_counter;
use aqbot_core::types::{ChatContent, ChatMessage};
use std::collections::HashSet;

/// Fraction of context window that triggers auto-compression (70%).
const THRESHOLD_RATIO: f64 = 0.70;

/// Content string for the compression marker message.
pub const COMPRESSION_MARKER: &str = "<!-- context-compressed -->";

/// Default number of trailing compressible messages to leave out of compression.
pub const DEFAULT_COMPRESSION_KEEP_LAST_N: u32 = 3;

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
    let text = match &msg.content {
        ChatContent::Text(s) => s.as_str(),
        ChatContent::Multipart(parts) => {
            return token_counter::estimate_tokens(
                &parts
                    .iter()
                    .filter_map(|p| p.text.as_deref())
                    .collect::<Vec<_>>()
                    .join(" "),
            ) + parts.iter().filter(|p| p.image_url.is_some()).count() * 85
                + 4;
        }
    };
    token_counter::estimate_message_tokens(&msg.role, text)
}

/// Check whether the current context exceeds the auto-compression threshold.
///
/// Returns `true` if total tokens (system + history) > model_context_window * 0.70.
///
/// When `model_context_window` is `None` (model has no configured limit), always
/// returns `false` — we never auto-compress without a known budget.
pub fn should_auto_compress(
    system_messages: &[ChatMessage],
    history_messages: &[ChatMessage],
    model_context_window: Option<u32>,
) -> bool {
    let context_window = match model_context_window {
        Some(v) => v as usize,
        None => return false,
    };
    let threshold = (context_window as f64 * THRESHOLD_RATIO) as usize;

    let total: usize = system_messages
        .iter()
        .chain(history_messages.iter())
        .map(|m| message_tokens(m))
        .sum();

    total > threshold
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
pub fn apply_message_count_limit(
    history: &[ChatMessage],
    limit: Option<u32>,
) -> Vec<ChatMessage> {
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

/// Build the final context for LLM from system messages + optional summary + history.
///
/// If a summary exists, it is prepended as a system message.
/// Sliding window is applied only when `model_context_window` is `Some`.
/// When the model has no configured limit, all history messages are included.
pub fn build_context(
    system_messages: &[ChatMessage],
    history_messages: &[ChatMessage],
    existing_summary: Option<&str>,
    model_context_window: Option<u32>,
) -> Vec<ChatMessage> {
    let mut out = system_messages.to_vec();

    // Insert summary as a system message if present
    if let Some(summary_text) = existing_summary {
        out.push(ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text(format!(
                "[对话历史摘要 / Conversation History Summary]\n{}",
                summary_text
            )),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    match model_context_window {
        Some(ctx_window) => {
            let budget = (ctx_window as f64 * THRESHOLD_RATIO) as usize;
            let system_tokens: usize = out.iter().map(|m| message_tokens(m)).sum();
            let available = budget.saturating_sub(system_tokens);
            let trimmed = sliding_window(history_messages, available);
            out.extend(trimmed);
        }
        None => {
            // No known context limit — include all history messages
            out.extend(history_messages.iter().cloned());
        }
    }

    out
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
    let content_text = match &m.content {
        ChatContent::Text(s) => s.clone(),
        ChatContent::Multipart(parts) => parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join(" "),
    };
    let truncated = if content_text.len() > 2000 {
        format!("{}...[已截断]", &content_text[..2000])
    } else {
        content_text
    };
    format!("{}: {}", m.role, truncated)
}

fn default_compression_instruction(has_existing_summary: bool) -> &'static str {
    if has_existing_summary {
        "你是一个对话摘要助手。请将以下新增对话内容合并到已有摘要中。\n\n\
         要求：\n\
         1. 保留所有用户明确表达的需求、偏好和决策\n\
         2. 保留关键技术细节（代码片段、配置、错误信息等）\n\
         3. 保留待办事项和未解决的问题\n\
         4. 用简洁的要点形式组织\n\
         5. 如果有冲突信息，以最新的为准\n\
         6. 保持摘要简洁，不超过 500 字"
    } else {
        "你是一个对话摘要助手。请将以下对话历史压缩为简洁摘要。\n\n\
         要求：\n\
         1. 保留所有用户明确表达的需求、偏好和决策\n\
         2. 保留关键技术细节（代码片段、配置、错误信息等）\n\
         3. 保留待办事项和未解决的问题\n\
         4. 用简洁的要点形式组织\n\
         5. 保持摘要简洁，不超过 500 字"
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

/// Rebuild a compression prompt from stored `source_text` (retry path).
pub fn build_summary_prompt_from_source(source_text: &str, system_prompt: &str) -> Vec<ChatMessage> {
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
            content: ChatContent::Text(format!(
                "{}{}",
                source_text, COMPRESSION_FOOTER_REMINDER
            )),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
    ]
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
    use aqbot_core::types::{ToolCall, ToolCallFunction};

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
    fn deepseek_v4_flash_budget_does_not_auto_compress_below_threshold() {
        let history = vec![text_message("user", &"token ".repeat(100_000))];

        assert!(!should_auto_compress(&[], &history, Some(1_000_000)));
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
        assert_eq!(
            resolve_message_count_limit(Some(1), Some(10)),
            Some(1)
        );
        assert_eq!(
            resolve_message_count_limit(None, Some(3)),
            Some(3)
        );
        assert_eq!(resolve_message_count_limit(None, None), None);
        assert_eq!(
            resolve_message_count_limit(Some(50), Some(3)),
            None
        );
        assert_eq!(
            resolve_message_count_limit(None, Some(50)),
            None
        );
        assert_eq!(
            resolve_message_count_limit(Some(0), None),
            Some(0)
        );
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
        assert_eq!(
            apply_message_count_limit(&history, Some(50)).len(),
            5
        );

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
}
