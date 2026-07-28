use aqbot_providers::image_adapters::{
    parse_response_payload, GenericImageMapping, ImageAdapterConfig, ParsedImageSource,
    ParsedResponsePayload,
};

#[test]
fn parses_openai_glm_and_siliconflow_response_shapes() {
    let config = ImageAdapterConfig::default();
    let openai = parse_response_payload(
        "openai_images",
        &serde_json::json!({"id":"r1","data":[{"b64_json":"aGVsbG8="}]}),
        &config,
    )
    .expect("parse openai");
    assert!(matches!(
        openai,
        ParsedResponsePayload::Completed(ref value)
            if matches!(value.images[0].source, ParsedImageSource::Base64(ref data) if data == "aGVsbG8=")
    ));

    let glm = parse_response_payload(
        "glm_images",
        &serde_json::json!({"data":[{"url":"https://example.test/glm.png"}]}),
        &config,
    )
    .expect("parse glm");
    assert!(matches!(
        glm,
        ParsedResponsePayload::Completed(ref value)
            if matches!(value.images[0].source, ParsedImageSource::Url(ref url) if url.contains("glm.png"))
    ));

    let silicon = parse_response_payload(
        "siliconflow_images",
        &serde_json::json!({"images":[{"url":"https://example.test/sf.png"}]}),
        &config,
    )
    .expect("parse siliconflow");
    assert!(matches!(
        silicon,
        ParsedResponsePayload::Completed(ref value)
            if matches!(value.images[0].source, ParsedImageSource::Url(ref url) if url.contains("sf.png"))
    ));
}

#[test]
fn parses_gemini_inline_image_output() {
    let parsed = parse_response_payload(
        "gemini_images",
        &serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}}]
                }
            }]
        }),
        &ImageAdapterConfig::default(),
    )
    .expect("parse gemini");

    assert!(matches!(
        parsed,
        ParsedResponsePayload::Completed(ref value)
            if value.images.len() == 1
                && value.images[0].declared_mime_type.as_deref() == Some("image/png")
    ));
}

#[test]
fn parses_gemini_snake_case_inline_image_mime_type() {
    let parsed = parse_response_payload(
        "gemini_images",
        &serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"inline_data":{"mime_type":"image/webp","data":"aGVsbG8="}}]
                }
            }]
        }),
        &ImageAdapterConfig::default(),
    )
    .expect("parse gemini");

    assert!(matches!(
        parsed,
        ParsedResponsePayload::Completed(ref value)
            if value.images[0].declared_mime_type.as_deref() == Some("image/webp")
    ));
}

#[test]
fn parses_gemini_interactions_and_imagen_mime_types() {
    let interactions = parse_response_payload(
        "gemini_images",
        &serde_json::json!({
            "steps": [{
                "type": "model_output",
                "content": [{
                    "type": "image",
                    "mime_type": "image/jpeg",
                    "data": "aGVsbG8="
                }]
            }]
        }),
        &ImageAdapterConfig::default(),
    )
    .expect("parse Gemini Interactions image");
    assert!(matches!(
        interactions,
        ParsedResponsePayload::Completed(ref value)
            if value.images[0].declared_mime_type.as_deref() == Some("image/jpeg")
    ));

    let imagen = parse_response_payload(
        "gemini_images",
        &serde_json::json!({
            "predictions": [{
                "bytesBase64Encoded": "aGVsbG8=",
                "mimeType": "image/png"
            }]
        }),
        &ImageAdapterConfig::default(),
    )
    .expect("parse Imagen response");
    assert!(matches!(
        imagen,
        ParsedResponsePayload::Completed(ref value)
            if value.images[0].declared_mime_type.as_deref() == Some("image/png")
    ));
}

#[test]
fn parses_xai_and_generic_declared_mime_types() {
    let xai = parse_response_payload(
        "xai_images",
        &serde_json::json!({
            "data": [{
                "b64_json": "aGVsbG8=",
                "mime_type": "image/webp"
            }]
        }),
        &ImageAdapterConfig::default(),
    )
    .expect("parse xAI response");
    assert!(matches!(
        xai,
        ParsedResponsePayload::Completed(ref value)
            if value.images[0].declared_mime_type.as_deref() == Some("image/webp")
    ));

    let mut config = ImageAdapterConfig::default();
    config.mapping = GenericImageMapping {
        image_mime_type_path: Some("/content_type".into()),
        ..GenericImageMapping::default()
    };
    let generic = parse_response_payload(
        "generic_json",
        &serde_json::json!({
            "data": [{
                "b64_json": "aGVsbG8=",
                "content_type": "image/png"
            }]
        }),
        &config,
    )
    .expect("parse generic MIME mapping");
    assert!(matches!(
        generic,
        ParsedResponsePayload::Completed(ref value)
            if value.images[0].declared_mime_type.as_deref() == Some("image/png")
    ));
}

#[test]
fn generic_json_supports_json_pointer_and_pending_status_mapping() {
    let mut config = ImageAdapterConfig::default();
    config.mapping = GenericImageMapping {
        images_path: Some("/result/images".into()),
        image_url_path: Some("/url".into()),
        task_id_path: Some("/task/id".into()),
        status_path: Some("/task/status".into()),
        success_statuses: vec!["done".into()],
        failure_statuses: vec!["failed".into()],
        pending_statuses: vec!["queued".into(), "running".into()],
        ..GenericImageMapping::default()
    };
    let pending = parse_response_payload(
        "generic_json",
        &serde_json::json!({"task":{"id":"job-1","status":"queued"}}),
        &config,
    )
    .expect("parse pending generic response");
    assert!(matches!(
        pending,
        ParsedResponsePayload::Pending(ref value) if value.remote_task_id == "job-1"
    ));

    let unknown = parse_response_payload(
        "generic_json",
        &serde_json::json!({"task":{"id":"job-2","status":"mystery"}}),
        &config,
    )
    .expect_err("unknown terminal-less status without task id should fail");
    assert!(unknown.to_string().contains("Unknown image task status"));
}
