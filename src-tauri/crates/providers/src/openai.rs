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
        // Keep every /models entry. Image-family parameter profiles are resolved
        // later; they must not decide whether a remote model is visible.
        self.inner.list_models(ctx).await
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
