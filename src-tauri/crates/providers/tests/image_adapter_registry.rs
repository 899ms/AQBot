use aqbot_core::types::ProviderType;
use aqbot_providers::image_adapters::{
    ImageAdapterConfig, ImageAdapterRegistry, ImageModelDescriptor, ImageOperation,
    ImageParameterDescriptor, ImageParameterKind,
};

#[test]
fn infers_xai_profile_for_custom_grok_image_models() {
    let registry = ImageAdapterRegistry::new();
    let resolved = registry
        .resolve(&ProviderType::Custom, "grok-imagine-image", None)
        .expect("custom grok image model should resolve");

    assert_eq!(resolved.id(), "xai_images");
    assert!(resolved
        .descriptor("grok-imagine-image", &ImageAdapterConfig::default())
        .operations
        .contains(&ImageOperation::Generate));
}

#[test]
fn infers_xai_profile_for_grok_image_aliases_across_provider_types() {
    let registry = ImageAdapterRegistry::new();
    let config = ImageAdapterConfig::default();

    for (provider_type, model_id) in [
        (ProviderType::Custom, "grok-image"),
        (ProviderType::OpenAI, "grok-image"),
        (ProviderType::OpenAI, "grok-imagine-image-quality"),
        (ProviderType::Custom, "grok-imagine-image-pro"),
        (ProviderType::XAI, "grok-imagine-image"),
    ] {
        let resolved = registry
            .resolve(&provider_type, model_id, None)
            .unwrap_or_else(|| panic!("{provider_type:?}/{model_id} should resolve"));
        assert_eq!(resolved.id(), "xai_images", "{provider_type:?}/{model_id}");
        let descriptor = resolved.descriptor(model_id, &config);
        assert!(
            descriptor.warnings.is_empty(),
            "{provider_type:?}/{model_id} should have a verified profile"
        );
        assert!(descriptor.operations.contains(&ImageOperation::Generate));
        assert!(descriptor.operations.contains(&ImageOperation::Edit));
        assert!(!descriptor.operations.contains(&ImageOperation::MaskEdit));
        assert_eq!(descriptor.max_reference_images, 3);
        assert_eq!(descriptor.max_batch_size, 10);
        let keys: Vec<_> = descriptor
            .parameters
            .iter()
            .map(|parameter| parameter.key.as_str())
            .collect();
        assert!(keys.contains(&"aspect_ratio"), "{keys:?}");
        assert!(keys.contains(&"resolution"), "{keys:?}");
        assert!(keys.contains(&"n"), "{keys:?}");
    }
}

#[test]
fn infers_xai_profile_from_official_api_host() {
    let registry = ImageAdapterRegistry::new();
    let resolved = registry
        .resolve_with_host(
            &ProviderType::OpenAI,
            "custom-image-model",
            None,
            Some("https://api.x.ai/v1"),
        )
        .expect("xAI host should prefer xai_images");
    assert_eq!(resolved.id(), "xai_images");
    assert!(resolved
        .descriptor("custom-image-model", &ImageAdapterConfig::default())
        .warnings
        .is_empty());
}

#[test]
fn explicit_adapter_override_wins_over_provider_inference() {
    let registry = ImageAdapterRegistry::new();
    let config = ImageAdapterConfig {
        adapter_id: Some("generic_json".into()),
        ..ImageAdapterConfig::default()
    };
    let resolved = registry
        .resolve(&ProviderType::OpenAI, "gpt-image-2", Some(&config))
        .expect("explicit generic profile should resolve");

    assert_eq!(resolved.id(), "generic_json");
}

#[test]
fn descriptors_do_not_claim_unsupported_operations() {
    let registry = ImageAdapterRegistry::new();
    let config = ImageAdapterConfig::default();
    let glm = registry
        .resolve(&ProviderType::GLM, "cogview-4", None)
        .expect("glm adapter should resolve");
    let descriptor = glm.descriptor("cogview-4", &config);

    assert_eq!(descriptor.operations, vec![ImageOperation::Generate]);
}

#[test]
fn capability_overrides_can_narrow_but_not_expand_a_profile() {
    let registry = ImageAdapterRegistry::new();
    let glm = registry
        .resolve(&ProviderType::GLM, "cogview-4", None)
        .expect("glm adapter should resolve");
    let config = ImageAdapterConfig {
        operation_overrides: Some(vec![ImageOperation::Generate, ImageOperation::Edit]),
        ..Default::default()
    };

    assert_eq!(
        glm.descriptor("cogview-4", &config).operations,
        vec![ImageOperation::Generate]
    );
}

#[test]
fn generic_models_are_conservative_without_an_explicit_descriptor() {
    let registry = ImageAdapterRegistry::new();
    let adapter = registry
        .resolve(&ProviderType::Custom, "vendor-image-model", None)
        .expect("custom image model should resolve to generic adapter");
    let descriptor = adapter.descriptor(
        "vendor-image-model",
        &ImageAdapterConfig::default(),
    );

    assert_eq!(adapter.id(), "generic_json");
    assert_eq!(descriptor.operations, vec![ImageOperation::Generate]);
    assert!(descriptor.parameters.is_empty());
    assert_eq!(descriptor.max_batch_size, 1);
    assert_eq!(descriptor.max_reference_images, 0);
}

#[test]
fn generic_descriptor_override_explicitly_enables_extra_capabilities() {
    let registry = ImageAdapterRegistry::new();
    let adapter = registry
        .resolve(&ProviderType::Custom, "vendor-image-model", None)
        .expect("custom image model should resolve");
    let config = ImageAdapterConfig {
        descriptor_override: Some(ImageModelDescriptor {
            adapter_id: "ignored".into(),
            operations: vec![ImageOperation::Generate, ImageOperation::Edit],
            parameters: vec![ImageParameterDescriptor {
                key: "style".into(),
                kind: ImageParameterKind::Select,
                default: "natural".into(),
                options: vec!["natural".into(), "vivid".into()],
                min: None,
                max: None,
            }],
            max_batch_size: 2,
            max_reference_images: 1,
            warnings: vec![],
        }),
        ..Default::default()
    };
    let descriptor = adapter.descriptor("vendor-image-model", &config);

    assert_eq!(descriptor.adapter_id, "generic_json");
    assert!(descriptor.operations.contains(&ImageOperation::Edit));
    assert_eq!(descriptor.parameters[0].key, "style");
}
