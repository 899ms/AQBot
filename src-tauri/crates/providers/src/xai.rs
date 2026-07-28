use aqbot_core::error::Result;
use aqbot_core::types::*;
use async_trait::async_trait;
use futures::Stream;
use serde::Deserialize;
use std::collections::HashSet;
use std::pin::Pin;

use crate::openai_compat::{OpenAICompatAdapter, OpenAICompatPolicy};
use crate::reasoning::{ReasoningStyle, ResolvedReasoning};
use crate::{ProviderAdapter, ProviderRequestContext};

pub struct XAIAdapter {
    inner: OpenAICompatAdapter<XAIPolicy>,
}

#[derive(Clone, Copy)]
pub(crate) struct XAIPolicy;

fn supports_reasoning_effort(model: &str) -> bool {
    model.to_ascii_lowercase().starts_with("grok-4.3")
}

impl OpenAICompatPolicy for XAIPolicy {
    fn default_base_url(&self) -> &'static str {
        "https://api.x.ai/v1"
    }

    fn error_label(&self) -> &'static str {
        "xAI API"
    }

    fn default_reasoning_style(&self, request: &ChatRequest) -> ReasoningStyle {
        if supports_reasoning_effort(&request.model) {
            ReasoningStyle::OpenAIReasoningEffort
        } else {
            ReasoningStyle::None
        }
    }

    fn normalize_reasoning_effort(
        &self,
        request: &ChatRequest,
        _level: &str,
        effort: String,
    ) -> Option<String> {
        if !supports_reasoning_effort(&request.model) {
            return None;
        }
        matches!(effort.as_str(), "none" | "low" | "medium" | "high").then_some(effort)
    }

    fn use_max_completion_tokens(&self, _request: &ChatRequest) -> bool {
        true
    }

    fn suppress_sampling_params(&self, _reasoning: Option<&ResolvedReasoning>) -> bool {
        false
    }
}

impl XAIAdapter {
    pub fn new() -> Self {
        Self {
            inner: OpenAICompatAdapter::new(XAIPolicy),
        }
    }

    async fn list_image_models(&self, ctx: &ProviderRequestContext) -> Result<Vec<Model>> {
        let base_url = ctx
            .base_url
            .clone()
            .unwrap_or_else(|| XAIPolicy.default_base_url().to_string());
        let client = crate::build_http_client(ctx.proxy_config.as_ref())?;
        let response = crate::apply_request_headers(
            client
                .get(format!(
                    "{}/image-generation-models",
                    base_url.trim_end_matches('/')
                ))
                .bearer_auth(&ctx.api_key),
            ctx,
        )
        .send()
        .await
        .map_err(|error| {
            aqbot_core::error::AQBotError::Provider(format!(
                "xAI image model discovery failed: {error}"
            ))
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(aqbot_core::error::AQBotError::Provider(format!(
                "xAI image model discovery failed ({status}): {body}"
            )));
        }
        let payload: XaiImageModelsResponse = response.json().await.map_err(|error| {
            aqbot_core::error::AQBotError::Provider(format!(
                "xAI image model discovery response was invalid: {error}"
            ))
        })?;
        Ok(parse_xai_image_models(&ctx.provider_id, payload))
    }
}

#[derive(Debug, Deserialize)]
struct XaiImageModelsResponse {
    #[serde(default)]
    models: Vec<XaiImageModel>,
}

#[derive(Debug, Deserialize)]
struct XaiImageModel {
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
}

fn parse_xai_image_models(provider_id: &str, payload: XaiImageModelsResponse) -> Vec<Model> {
    payload
        .models
        .into_iter()
        .flat_map(|model| {
            let primary = model.id;
            std::iter::once(primary.clone())
                .chain(model.aliases)
                .map(move |model_id| Model {
                    provider_id: provider_id.to_string(),
                    name: model_id.clone(),
                    model_id,
                    group_name: Some("grok-imagine".into()),
                    model_type: ModelType::Image,
                    capabilities: vec![],
                    context_window: None,
                    max_output_tokens: None,
                    enabled: true,
                    param_overrides: None,
                    image_config: None,
                    metadata_state: None,
                })
        })
        .collect()
}

fn merge_image_models(mut models: Vec<Model>, image_models: Vec<Model>) -> Vec<Model> {
    let mut positions = models
        .iter()
        .enumerate()
        .map(|(index, model)| (model.model_id.clone(), index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut seen = HashSet::new();
    for image_model in image_models {
        if !seen.insert(image_model.model_id.clone()) {
            continue;
        }
        if let Some(index) = positions.get(&image_model.model_id).copied() {
            models[index].model_type = ModelType::Image;
            models[index].capabilities.clear();
        } else {
            positions.insert(image_model.model_id.clone(), models.len());
            models.push(image_model);
        }
    }
    models
}

#[async_trait]
impl ProviderAdapter for XAIAdapter {
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
        let (models, image_models) =
            tokio::try_join!(self.inner.list_models(ctx), self.list_image_models(ctx))?;
        Ok(merge_image_models(models, image_models))
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
mod image_model_tests {
    use super::*;

    #[test]
    fn official_image_model_response_preserves_ids_and_aliases() {
        let parsed = parse_xai_image_models(
            "xai",
            XaiImageModelsResponse {
                models: vec![XaiImageModel {
                    id: "grok-imagine-image".into(),
                    aliases: vec!["grok-imagine-image-latest".into()],
                }],
            },
        );
        assert_eq!(parsed.len(), 2);
        assert!(parsed
            .iter()
            .all(|model| model.model_type == ModelType::Image));
    }
}
