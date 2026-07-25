use aqbot_core::db::create_test_pool;
use aqbot_core::repo::provider;
use aqbot_core::types::{CreateProviderInput, ProviderType, UpdateProviderInput};

#[tokio::test]
async fn bedrock_provider_requires_and_persists_region() {
    let harness = create_test_pool().await.unwrap();
    let db = &harness.conn;
    let clear_region_input: UpdateProviderInput =
        serde_json::from_value(serde_json::json!({ "aws_region": null })).unwrap();
    assert_eq!(clear_region_input.aws_region, Some(None));

    let missing_region = provider::create_provider(
        db,
        CreateProviderInput {
            name: "Bedrock invalid".into(),
            provider_type: ProviderType::Bedrock,
            api_host: String::new(),
            api_path: None,
            aws_region: None,
            enabled: true,
            builtin_id: None,
        },
    )
    .await;
    assert!(missing_region.is_err());

    let created = provider::create_provider(
        db,
        CreateProviderInput {
            name: "AWS Bedrock".into(),
            provider_type: ProviderType::Bedrock,
            api_host: String::new(),
            api_path: None,
            aws_region: Some(" us-west-2 ".into()),
            enabled: true,
            builtin_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(created.provider_type, ProviderType::Bedrock);
    assert_eq!(created.aws_region.as_deref(), Some("us-west-2"));

    let updated = provider::update_provider(
        db,
        &created.id,
        UpdateProviderInput {
            aws_region: Some(Some("eu-central-1".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.aws_region.as_deref(), Some("eu-central-1"));

    let clear_region = provider::update_provider(
        db,
        &created.id,
        UpdateProviderInput {
            aws_region: Some(None),
            ..Default::default()
        },
    )
    .await;
    assert!(clear_region.is_err());

    let openai = provider::create_provider(
        db,
        CreateProviderInput {
            name: "OpenAI".into(),
            provider_type: ProviderType::OpenAI,
            api_host: "https://api.openai.com".into(),
            api_path: None,
            aws_region: Some("us-east-1".into()),
            enabled: true,
            builtin_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(openai.aws_region, None);

    let convert_openai = provider::update_provider(
        db,
        &openai.id,
        UpdateProviderInput {
            provider_type: Some(ProviderType::Bedrock),
            aws_region: Some(Some("us-east-1".into())),
            ..Default::default()
        },
    )
    .await;
    assert!(convert_openai.is_err());

    let convert_bedrock = provider::update_provider(
        db,
        &created.id,
        UpdateProviderInput {
            provider_type: Some(ProviderType::OpenAI),
            ..Default::default()
        },
    )
    .await;
    assert!(convert_bedrock.is_err());
}
