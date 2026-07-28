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
        let id = config
            .and_then(|value| value.adapter_id.as_deref())
            .unwrap_or_else(|| infer_adapter_id(provider_type, model_id));
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

fn infer_adapter_id(provider_type: &ProviderType, model_id: &str) -> &'static str {
    let normalized = model_id.to_ascii_lowercase();
    match provider_type {
        ProviderType::XAI => "xai_images",
        ProviderType::GLM => "glm_images",
        ProviderType::SiliconFlow => "siliconflow_images",
        ProviderType::Gemini => "gemini_images",
        ProviderType::Custom if normalized.starts_with("grok-imagine") => "xai_images",
        ProviderType::Custom
            if normalized.starts_with("gpt-image-") || normalized.starts_with("dall-e-") =>
        {
            "openai_images"
        }
        ProviderType::Custom => "generic_json",
        ProviderType::OpenAI => "openai_images",
        _ => "generic_json",
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
}
