use aqbot_core::error::Result;
use aqbot_core::types::*;
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::openai_compat::{OpenAICompatAdapter, OpenAICompatPolicy};
use crate::reasoning::{ReasoningStyle, ResolvedReasoning};
use crate::{ProviderAdapter, ProviderRequestContext};

pub struct OpenAIAdapter {
    inner: OpenAICompatAdapter<OpenAIPolicy>,
}

#[derive(Clone, Copy)]
pub(crate) struct OpenAIPolicy;

impl OpenAICompatPolicy for OpenAIPolicy {
    fn max_completion_tokens_cap(&self, request: &ChatRequest) -> Option<u32> {
        crate::deepseek::deepseek_compat_max_completion_tokens_cap(request)
    }

    fn suppress_sampling_params(&self, reasoning: Option<&ResolvedReasoning>) -> bool {
        reasoning.is_some_and(|r| {
            matches!(
                r.style,
                ReasoningStyle::OpenAIReasoningEffort | ReasoningStyle::OpenAIResponsesReasoning
            ) && !matches!(r.level.as_str(), "off" | "none")
                && r.suppress_sampling_params
        })
    }
}

impl OpenAIAdapter {
    pub fn new() -> Self {
        Self {
            inner: OpenAICompatAdapter::new(OpenAIPolicy),
        }
    }
}

fn is_official_openai_image_model(model_id: &str) -> bool {
    let normalized = model_id.to_ascii_lowercase();
    normalized.starts_with("gpt-image-2")
        || normalized.starts_with("gpt-image-1.5")
        || normalized.starts_with("gpt-image-1-mini")
        || normalized == "gpt-image-1"
        || normalized.starts_with("gpt-image-1-")
        || normalized.starts_with("dall-e-2")
        || normalized.starts_with("dall-e-3")
}

#[async_trait]
impl ProviderAdapter for OpenAIAdapter {
    async fn chat(
        &self,
        ctx: &ProviderRequestContext,
        request: ChatRequest,
    ) -> Result<ChatResponse> {
        self.inner.chat(ctx, request).await
    }

    fn chat_stream(
        &self,
        ctx: &ProviderRequestContext,
        request: ChatRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>> {
        self.inner.chat_stream(ctx, request)
    }

    async fn list_models(&self, ctx: &ProviderRequestContext) -> Result<Vec<Model>> {
        let mut models = self.inner.list_models(ctx).await?;
        models.retain(|model| {
            model.model_type != ModelType::Image || is_official_openai_image_model(&model.model_id)
        });
        Ok(models)
    }

    async fn embed(
        &self,
        ctx: &ProviderRequestContext,
        request: EmbedRequest,
    ) -> Result<EmbedResponse> {
        self.inner.embed(ctx, request).await
    }

    async fn validate_key(&self, ctx: &ProviderRequestContext) -> Result<bool> {
        self.inner.validate_key(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::is_official_openai_image_model;

    #[test]
    fn image_model_allowlist_accepts_current_and_legacy_image_api_models() {
        for model in [
            "gpt-image-2",
            "gpt-image-2-2026-07-01",
            "gpt-image-1.5",
            "gpt-image-1",
            "gpt-image-1-2025-04-15",
            "gpt-image-1-mini",
            "dall-e-2",
            "dall-e-3",
        ] {
            assert!(is_official_openai_image_model(model), "{model}");
        }
        assert!(!is_official_openai_image_model("chatgpt-image-latest"));
    }
}
