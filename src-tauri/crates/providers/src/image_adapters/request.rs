use super::{
    image_model_profile, resolved_gemini_api_mode, ImageAdapterConfig, ImageAdapterRequest,
    ImageApiMode, ImageModelFamily,
};
use aqbot_core::error::{AQBotError, Result};
use base64::Engine;
use serde_json::{Map, Value};

pub fn build_request_body(
    adapter_id: &str,
    request: &ImageAdapterRequest,
    config: &ImageAdapterConfig,
) -> Result<Value> {
    let family = image_model_profile(adapter_id, &request.model, config).family;
    let mut body = match adapter_id {
        "openai_images" => openai_body(request, family),
        "xai_images" => xai_body(request, family),
        "glm_images" => glm_body(request, family),
        "siliconflow_images" => siliconflow_body(request, family),
        "gemini_images" => gemini_body(request, config),
        "generic_json" => generic_body(request, config)?,
        other => {
            return Err(AQBotError::Validation(format!(
                "Unknown image adapter: {other}"
            )))
        }
    };
    merge_extra_body(&mut body, &config.extra_body)?;
    Ok(body)
}

fn openai_body(request: &ImageAdapterRequest, family: ImageModelFamily) -> Value {
    if family == ImageModelFamily::Unknown {
        return minimal_body(request);
    }
    let mut body = base_body(request);
    insert_string(&mut body, "size", &request.size, "auto");
    insert_string(&mut body, "quality", &request.quality, "auto");
    if !matches!(family, ImageModelFamily::DallE2 | ImageModelFamily::DallE3) {
        insert_string(&mut body, "output_format", &request.output_format, "");
        insert_optional_string(&mut body, "background", request.background.as_deref());
        if let Some(value) = request.output_compression {
            body.insert("output_compression".into(), value.into());
        }
    }
    Value::Object(body)
}

fn xai_body(request: &ImageAdapterRequest, family: ImageModelFamily) -> Value {
    if family == ImageModelFamily::Unknown {
        return minimal_body(request);
    }
    let mut body = Map::new();
    body.insert("model".into(), request.model.clone().into());
    body.insert("prompt".into(), request.prompt.clone().into());
    body.insert("n".into(), request.n.into());
    body.insert("response_format".into(), "b64_json".into());
    copy_parameter(request, &mut body, "aspect_ratio");
    copy_parameter(request, &mut body, "resolution");
    if request.images.len() == 1 {
        body.insert("image".into(), xai_image_reference(&request.images[0]));
    } else if request.images.len() > 1 {
        body.insert(
            "images".into(),
            request
                .images
                .iter()
                .map(xai_image_reference)
                .collect::<Vec<_>>()
                .into(),
        );
    }
    Value::Object(body)
}

fn xai_image_reference(image: &crate::openai_images::ImageUpload) -> Value {
    serde_json::json!({
        "type": "image_url",
        "url": upload_data_url(image),
    })
}

fn glm_body(request: &ImageAdapterRequest, family: ImageModelFamily) -> Value {
    if family == ImageModelFamily::Unknown {
        return minimal_body(request);
    }
    let mut body = Map::new();
    body.insert("model".into(), request.model.clone().into());
    body.insert("prompt".into(), request.prompt.clone().into());
    insert_string(&mut body, "size", &request.size, "auto");
    insert_string(&mut body, "quality", &request.quality, "auto");
    Value::Object(body)
}

fn siliconflow_body(request: &ImageAdapterRequest, family: ImageModelFamily) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), request.model.clone().into());
    body.insert("prompt".into(), request.prompt.clone().into());
    if matches!(
        family,
        ImageModelFamily::SiliconKolors | ImageModelFamily::SiliconQwen
    ) {
        insert_string(&mut body, "image_size", &request.size, "auto");
    }
    if matches!(family, ImageModelFamily::SiliconKolors) {
        body.insert("batch_size".into(), request.n.into());
    }
    let parameter_keys: &[&str] = match family {
        ImageModelFamily::SiliconKolors => &["seed", "num_inference_steps", "guidance_scale"],
        ImageModelFamily::SiliconQwen | ImageModelFamily::SiliconQwenEdit => {
            &["seed", "num_inference_steps"]
        }
        _ => &[],
    };
    for key in parameter_keys {
        copy_parameter(request, &mut body, key);
    }
    if matches!(family, ImageModelFamily::SiliconQwenEdit) {
        for (index, image) in request.images.iter().enumerate().take(3) {
            let key = if index == 0 {
                "image".to_string()
            } else {
                format!("image{}", index + 1)
            };
            body.insert(key, upload_data_url(image).into());
        }
    }
    Value::Object(body)
}

fn gemini_body(request: &ImageAdapterRequest, config: &ImageAdapterConfig) -> Value {
    match resolved_gemini_api_mode(&request.model, config) {
        ImageApiMode::Interactions => gemini_interactions_body(request),
        ImageApiMode::Predict => imagen_predict_body(request),
        ImageApiMode::Auto | ImageApiMode::GenerateContent => gemini_generate_content_body(request),
    }
}

fn gemini_interactions_body(request: &ImageAdapterRequest) -> Value {
    let mut input = vec![serde_json::json!({
        "type": "text",
        "text": request.prompt,
    })];
    input.extend(request.images.iter().map(|image| {
        serde_json::json!({
            "type": "image",
            "mime_type": image.mime_type,
            "data": base64::engine::general_purpose::STANDARD.encode(&image.bytes),
        })
    }));
    let mut response_format = serde_json::json!({
        "type": "image",
        "mime_type": output_format_to_mime(&request.output_format),
    });
    copy_parameter_to_value(request, &mut response_format, "aspect_ratio");
    copy_parameter_to_value(request, &mut response_format, "image_size");
    serde_json::json!({
        "model": request.model,
        "input": input,
        "store": false,
        "response_format": response_format,
    })
}

fn gemini_generate_content_body(request: &ImageAdapterRequest) -> Value {
    let mut parts = vec![serde_json::json!({ "text": request.prompt })];
    parts.extend(request.images.iter().map(|image| {
        serde_json::json!({
            "inlineData": {
                "mimeType": image.mime_type,
                "data": base64::engine::general_purpose::STANDARD.encode(&image.bytes),
            }
        })
    }));
    let mut body = serde_json::json!({
        "contents": [{ "parts": parts }],
        "generationConfig": { "responseModalities": ["TEXT", "IMAGE"] },
    });
    let mut image_format = serde_json::json!({
        "mimeType": output_format_to_mime(&request.output_format),
    });
    if let Some(aspect_ratio) = request.parameters.get("aspect_ratio") {
        image_format["aspectRatio"] = aspect_ratio.clone();
    }
    if let Some(image_size) = request.parameters.get("image_size") {
        image_format["imageSize"] = image_size.clone();
    }
    body["generationConfig"]["responseFormat"]["image"] = image_format;
    body
}

fn imagen_predict_body(request: &ImageAdapterRequest) -> Value {
    let mut parameters = serde_json::json!({ "sampleCount": request.n });
    copy_renamed_parameter_to_value(request, &mut parameters, "aspect_ratio", "aspectRatio");
    copy_renamed_parameter_to_value(request, &mut parameters, "image_size", "imageSize");
    copy_renamed_parameter_to_value(
        request,
        &mut parameters,
        "person_generation",
        "personGeneration",
    );
    serde_json::json!({
        "instances": [{ "prompt": request.prompt }],
        "parameters": parameters,
    })
}

fn generic_body(request: &ImageAdapterRequest, config: &ImageAdapterConfig) -> Result<Value> {
    let values = semantic_values(request);
    let mut body = Value::Object(Map::new());
    let mappings = if config.mapping.request_fields.is_empty() {
        default_generic_mappings()
    } else {
        config.mapping.request_fields.clone()
    };
    for (semantic, target) in mappings {
        if let Some(value) = values.get(&semantic) {
            set_dotted_path(&mut body, &target, value.clone())?;
        }
    }
    Ok(body)
}

fn semantic_values(request: &ImageAdapterRequest) -> Map<String, Value> {
    let mut values = base_body(request);
    values.insert("size".into(), request.size.clone().into());
    values.insert("quality".into(), request.quality.clone().into());
    values.insert("output_format".into(), request.output_format.clone().into());
    if !request.images.is_empty() {
        values.insert(
            "images".into(),
            request
                .images
                .iter()
                .map(upload_data_url)
                .collect::<Vec<_>>()
                .into(),
        );
    }
    if let Some(mask) = &request.mask {
        values.insert("mask".into(), upload_data_url(mask).into());
    }
    values.extend(
        request
            .parameters
            .iter()
            .filter(|(key, _)| !key.starts_with("_aqbot_"))
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    values
}

fn base_body(request: &ImageAdapterRequest) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("model".into(), request.model.clone().into());
    body.insert("prompt".into(), request.prompt.clone().into());
    body.insert("n".into(), request.n.into());
    body
}

fn minimal_body(request: &ImageAdapterRequest) -> Value {
    serde_json::json!({
        "model": request.model,
        "prompt": request.prompt,
    })
}

fn default_generic_mappings() -> std::collections::BTreeMap<String, String> {
    ["model", "prompt"]
        .into_iter()
        .map(|key| (key.to_string(), key.to_string()))
        .collect()
}

fn copy_parameter(request: &ImageAdapterRequest, body: &mut Map<String, Value>, key: &str) {
    if let Some(value) = request.parameters.get(key) {
        body.insert(key.to_string(), value.clone());
    }
}

fn copy_parameter_to_value(request: &ImageAdapterRequest, body: &mut Value, key: &str) {
    copy_renamed_parameter_to_value(request, body, key, key);
}

fn copy_renamed_parameter_to_value(
    request: &ImageAdapterRequest,
    body: &mut Value,
    source: &str,
    target: &str,
) {
    if let (Some(value), Some(object)) = (request.parameters.get(source), body.as_object_mut()) {
        object.insert(target.to_string(), value.clone());
    }
}

fn output_format_to_mime(output_format: &str) -> &'static str {
    match output_format {
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn set_dotted_path(root: &mut Value, path: &str, value: Value) -> Result<()> {
    let segments = path.split('.').collect::<Vec<_>>();
    if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        return Err(AQBotError::Validation(format!(
            "Invalid generic image field mapping: {path}"
        )));
    }
    let mut current = root;
    for segment in &segments[..segments.len() - 1] {
        current = current
            .as_object_mut()
            .ok_or_else(|| AQBotError::Validation("Image mapping target is not an object".into()))?
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    current
        .as_object_mut()
        .ok_or_else(|| AQBotError::Validation("Image mapping target is not an object".into()))?
        .insert(segments[segments.len() - 1].to_string(), value);
    Ok(())
}

fn merge_extra_body(body: &mut Value, extra: &Map<String, Value>) -> Result<()> {
    let target = body
        .as_object_mut()
        .ok_or_else(|| AQBotError::Validation("Image request body must be an object".into()))?;
    target.extend(extra.clone());
    Ok(())
}

fn insert_string(body: &mut Map<String, Value>, key: &str, value: &str, omitted: &str) {
    if !value.is_empty() && value != omitted {
        body.insert(key.into(), value.into());
    }
}

fn insert_optional_string(body: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty() && *value != "auto") {
        body.insert(key.into(), value.into());
    }
}

fn upload_data_url(upload: &crate::openai_images::ImageUpload) -> String {
    format!(
        "data:{};base64,{}",
        upload.mime_type,
        base64::engine::general_purpose::STANDARD.encode(&upload.bytes)
    )
}
