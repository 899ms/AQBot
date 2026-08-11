// Document attachment extraction and context injection.

fn document_attachment_char_limit(model_context_window: Option<u32>) -> usize {
    model_context_window
        .map(|tokens| (tokens as usize).saturating_mul(2))
        .unwrap_or(DOCUMENT_ATTACHMENT_UNKNOWN_CONTEXT_CHAR_LIMIT)
        .clamp(
            DOCUMENT_ATTACHMENT_MIN_CONTEXT_CHAR_LIMIT,
            DOCUMENT_ATTACHMENT_MAX_CONTEXT_CHAR_LIMIT,
        )
}

fn attachment_effective_mime_type(attachment: &Attachment) -> String {
    if !attachment.file_type.is_empty() && attachment.file_type != "application/octet-stream" {
        return attachment.file_type.clone();
    }
    aqbot_core::document_parser::mime_from_extension(std::path::Path::new(&attachment.file_name))
        .to_string()
}

fn is_supported_document_attachment(attachment: &Attachment) -> bool {
    matches!(
        attachment_effective_mime_type(attachment).as_str(),
        "application/pdf"
            | "application/msword"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "text/plain"
            | "text/markdown"
            | "text/csv"
            | "text/html"
            | "text/xml"
            | "application/json"
            | "application/xml"
    )
}

fn truncate_to_char_limit(text: &str, limit: usize) -> (String, bool) {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= limit {
            return (out, true);
        }
        out.push(ch);
    }
    (out, false)
}

fn read_document_attachment_text(
    file_store: &aqbot_core::file_store::FileStore,
    attachment: &Attachment,
) -> aqbot_core::error::Result<Option<String>> {
    let mime_type = attachment_effective_mime_type(attachment);
    if attachment.file_path.is_empty() {
        let Some(data) = attachment.data.as_ref() else {
            return Ok(None);
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| {
                aqbot_core::error::AQBotError::Validation(format!(
                    "Invalid attachment base64 for {}: {}",
                    attachment.file_name, e
                ))
            })?;
        let extension = std::path::Path::new(&attachment.file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("tmp");
        let temp_path = std::env::temp_dir().join(format!(
            "aqbot-doc-{}.{}",
            aqbot_core::utils::gen_id(),
            extension
        ));
        std::fs::write(&temp_path, bytes)?;
        let result = aqbot_core::document_parser::extract_text(&temp_path, &mime_type);
        let _ = std::fs::remove_file(&temp_path);
        return result.map(Some);
    }

    let path = file_store.validated_path(&attachment.file_path)?;
    if !path.exists() {
        return Ok(None);
    }
    aqbot_core::document_parser::extract_text(&path, &mime_type).map(Some)
}

pub(crate) fn append_document_attachment_context(
    file_store: &aqbot_core::file_store::FileStore,
    content: &str,
    attachments: &[Attachment],
    document_attachment_reading_enabled: bool,
    model_context_window: Option<u32>,
) -> aqbot_core::error::Result<String> {
    if !document_attachment_reading_enabled {
        return Ok(content.to_string());
    }

    let document_attachments = attachments
        .iter()
        .filter(|attachment| is_supported_document_attachment(attachment))
        .collect::<Vec<_>>();
    if document_attachments.is_empty() {
        return Ok(content.to_string());
    }

    let mut remaining_chars = document_attachment_char_limit(model_context_window);
    let mut blocks = Vec::new();
    for attachment in document_attachments {
        if remaining_chars == 0 {
            break;
        }
        let Some(text) = read_document_attachment_text(file_store, attachment)? else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (excerpt, truncated) = truncate_to_char_limit(trimmed, remaining_chars);
        remaining_chars = remaining_chars.saturating_sub(excerpt.chars().count());
        let mut quoted = excerpt
            .lines()
            .map(|line| format!("> {}", line))
            .collect::<Vec<_>>()
            .join("\n");
        if truncated {
            quoted.push_str("\n> [Document text truncated for model context budget.]");
        }
        blocks.push(format!(
            "Document attachment \"{}\":\n{}",
            attachment.file_name, quoted
        ));
    }

    if blocks.is_empty() {
        return Ok(content.to_string());
    }

    let mut result = content.trim_end().to_string();
    if !result.is_empty() {
        result.push_str("\n\n");
    }
    result.push_str("[Parsed document attachments]\n\n");
    result.push_str(&blocks.join("\n\n"));
    Ok(result)
}
