use super::types::{ImageAdapter, ImageAdapterConfig, ImageModelDescriptor};
use super::{
    cancel_profile, image_model_profile, poll_profile, submit_profile, validate_profile_request,
    ImageAdapterRequest, ImagePollResult, ImageSubmission, PendingImageSubmission,
};
use crate::ProviderRequestContext;
use aqbot_core::error::Result;
use aqbot_core::types::ProviderType;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ImageAdapterRegistry {
    adapters: HashMap<&'static str, Arc<dyn ImageAdapter>>,
}

impl ImageAdapterRegistry {
    pub fn new() -> Self {
        let mut adapters: HashMap<&'static str, Arc<dyn ImageAdapter>> = HashMap::new();
        for id in [
            "openai_images",
            "xai_images",
            "glm_images",
            "siliconflow_images",
            "gemini_images",
            "generic_json",
        ] {
            adapters.insert(id, Arc::new(ProfileImageAdapter { id }));
        }
        Self { adapters }
    }

    pub fn resolve(
        &self,
        provider_type: &ProviderType,
        model_id: &str,
        config: Option<&ImageAdapterConfig>,
    ) -> Option<Arc<dyn ImageAdapter>> {
        self.resolve_with_host(provider_type, model_id, config, None)
    }

    /// Resolve an image adapter, optionally using the provider API host for inference.
    ///
    /// Explicit `config.adapter_id` always wins. Otherwise well-known xAI Imagine model
    /// IDs and `api.x.ai` hosts route to `xai_images` even under OpenAI-compat/Custom types.
    pub fn resolve_with_host(
        &self,
        provider_type: &ProviderType,
        model_id: &str,
        config: Option<&ImageAdapterConfig>,
        api_host: Option<&str>,
    ) -> Option<Arc<dyn ImageAdapter>> {
        let id = config
            .and_then(|value| value.adapter_id.as_deref())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| infer_adapter_id(provider_type, model_id, api_host, config));
        self.adapters.get(id).cloned()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ImageAdapter>> {
        self.adapters.get(id).cloned()
    }
}

impl Default for ImageAdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Official and common alias IDs for xAI Imagine image models.
pub fn is_xai_image_model(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase();
    normalized.starts_with("grok-imagine") || normalized.starts_with("grok-image")
}

fn is_xai_api_host(api_host: &str) -> bool {
    api_host.to_ascii_lowercase().contains("api.x.ai")
}

fn is_official_gemini_host(api_host: &str) -> bool {
    api_host
        .to_ascii_lowercase()
        .contains("generativelanguage.googleapis.com")
}

fn looks_like_gemini_image_model(model_id: &str) -> bool {
    let normalized = model_id.to_ascii_lowercase();
    normalized.contains("gemini") && normalized.contains("image")
        || normalized.starts_with("imagen-")
        || normalized.contains("nano-banana")
}

/// True when the user configured a custom generic request/response mapping.
pub fn has_custom_image_mapping(config: &ImageAdapterConfig) -> bool {
    let mapping = &config.mapping;
    !mapping.request_fields.is_empty()
        || mapping.images_path.is_some()
        || mapping.image_url_path.is_some()
        || mapping.image_base64_path.is_some()
        || mapping.image_mime_type_path.is_some()
        || mapping.task_id_path.is_some()
        || mapping.status_path.is_some()
        || !mapping.success_statuses.is_empty()
        || !mapping.failure_statuses.is_empty()
        || !mapping.pending_statuses.is_empty()
}

fn infer_adapter_id(
    provider_type: &ProviderType,
    model_id: &str,
    api_host: Option<&str>,
    config: Option<&ImageAdapterConfig>,
) -> &'static str {
    let normalized = model_id.to_ascii_lowercase();
    // Preserve explicit generic setups that only set a mapping (no adapter_id).
    if config.is_some_and(has_custom_image_mapping) {
        return "generic_json";
    }
    if is_xai_image_model(&normalized) {
        return "xai_images";
    }
    if api_host.is_some_and(is_xai_api_host) {
        return "xai_images";
    }
    // Official Gemini host + Gemini/Imagen model names → native Gemini adapter.
    // Proxy hosts keep OpenAI Images so OpenAI-compatible Gemini relays work.
    if api_host.is_some_and(is_official_gemini_host) && looks_like_gemini_image_model(&normalized) {
        return "gemini_images";
    }
    match provider_type {
        ProviderType::XAI => "xai_images",
        ProviderType::GLM => "glm_images",
        ProviderType::SiliconFlow => "siliconflow_images",
        ProviderType::Gemini => "gemini_images",
        // OpenAI-compatible platforms (including Custom) default to OpenAI Images.
        // generic_json is only used when the user picks it or supplies a custom mapping.
        ProviderType::Custom | ProviderType::OpenAI => "openai_images",
        _ => "openai_images",
    }
}

struct ProfileImageAdapter {
    id: &'static str,
}

#[async_trait]
impl ImageAdapter for ProfileImageAdapter {
    fn id(&self) -> &'static str {
        self.id
    }

    fn descriptor(&self, model_id: &str, config: &ImageAdapterConfig) -> ImageModelDescriptor {
        image_model_profile(self.id, model_id, config).descriptor
    }

    fn validate_request(
        &self,
        request: &ImageAdapterRequest,
        reference_count: usize,
        config: &ImageAdapterConfig,
    ) -> Result<()> {
        validate_profile_request(self.id, request, reference_count, config)
    }

    async fn submit(
        &self,
        ctx: &ProviderRequestContext,
        request: ImageAdapterRequest,
        config: &ImageAdapterConfig,
    ) -> Result<ImageSubmission> {
        submit_profile(self.id, ctx, request, config).await
    }

    async fn poll(
        &self,
        ctx: &ProviderRequestContext,
        task: &PendingImageSubmission,
        config: &ImageAdapterConfig,
    ) -> Result<ImagePollResult> {
        poll_profile(self.id, ctx, task, config).await
    }

    async fn cancel(
        &self,
        ctx: &ProviderRequestContext,
        task: &PendingImageSubmission,
        config: &ImageAdapterConfig,
    ) -> Result<()> {
        cancel_profile(self.id, ctx, task, config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_descriptor_contains_protocol_metadata_without_display_labels() {
        let adapter = ImageAdapterRegistry::default()
            .get("openai_images")
            .expect("openai_images adapter");
        let descriptor = adapter.descriptor("gpt-image-2", &ImageAdapterConfig::default());
        let serialized = serde_json::to_value(descriptor).expect("serialize descriptor");
        let parameters = serialized["parameters"]
            .as_array()
            .expect("descriptor parameters");

        assert!(parameters
            .iter()
            .all(|parameter| parameter.get("label").is_none()));
        assert_eq!(parameters[0]["kind"], "string");
        assert!(serialized["warnings"].is_array());
    }

    #[test]
    fn xai_provider_routes_compat_image_ids_to_xai_images() {
        let registry = ImageAdapterRegistry::default();
        let adapter = registry
            .resolve(&ProviderType::XAI, "x-image", None)
            .expect("xAI image adapter");
        assert_eq!(adapter.id(), "xai_images");
    }
}
