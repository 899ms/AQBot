#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{PromptCapabilities, StringPropertySchema};

    fn form_request(
        message: &str,
        requested_schema: serde_json::Value,
    ) -> CreateElicitationRequest {
        serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "toolCallId": "question-1",
            "mode": "form",
            "message": message,
            "requestedSchema": requested_schema,
        }))
        .expect("valid test elicitation")
    }

    fn normalized_form(
        request: &CreateElicitationRequest,
    ) -> Result<(serde_json::Value, ElicitationFormContext), String> {
        let ElicitationMode::Form(form) = &request.mode else {
            panic!("test request uses form mode");
        };
        normalize_elicitation_form(request, form)
    }

    #[test]
    fn initialize_advertises_form_elicitation_but_not_unimplemented_plan_operations() {
        let initialize =
            serde_json::to_value(aqbot_initialize_request()).expect("serialize initialize request");
        assert_eq!(
            initialize.pointer("/clientCapabilities/elicitation/form"),
            Some(&serde_json::json!({}))
        );
        assert!(initialize
            .pointer("/clientCapabilities/session/plan")
            .is_none());
    }

    #[test]
    fn claude_companion_is_optional_and_custom_answer_wins() {
        let request = form_request(
            "Choose a deployment target",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "title": "Target",
                        "minLength": 1,
                        "oneOf": [{ "const": "cloud", "title": "Cloud" }]
                    },
                    "target_custom": {
                        "type": "string",
                        "title": "Custom",
                        "_meta": {
                            "_askUserQuestionCustomAnswer": {
                                "questionId": "target",
                                "isCustomAnswer": true
                            }
                        }
                    }
                }
            }),
        );
        let (raw, context) = normalized_form(&request).expect("normalize Claude form");
        assert_eq!(context.questions.len(), 1);
        assert!(!context.questions[0].required, "Claude permits skipping");
        assert_eq!(raw["questions"][0]["question"], request.message);
        assert_eq!(raw["questions"][0]["allowOther"], true);
        assert_eq!(raw["questions"][0]["minLength"], 1);

        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![0],
                other_text: Some("on-prem".into()),
            }],
        };
        let (_, response) = elicitation_response_from_submission(&context, &submission)
            .expect("custom answer overrides selected option");
        assert_eq!(
            serde_json::to_value(response).expect("serialize accepted form"),
            serde_json::json!({
                "action": "accept",
                "content": { "target_custom": "on-prem" }
            })
        );
    }

    #[test]
    fn codex_other_union_is_required_and_secret_default_never_leaks() {
        let request = form_request(
            "Enter the token",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "token": {
                        "type": "string",
                        "title": "Token",
                        "default": "managed",
                        "_meta": { "codex": { "isOther": true, "isSecret": true } },
                        "oneOf": [{ "const": "managed", "title": "Managed" }]
                    },
                    "token__other": {
                        "type": "string",
                        "_meta": { "codex": {
                            "questionId": "token",
                            "isOtherAnswer": true,
                            "isSecret": true
                        } }
                    }
                }
            }),
        );
        let (raw, context) = normalized_form(&request).expect("normalize Codex form");
        assert!(context.questions[0].required);
        assert!(raw["questions"][0].get("default").is_none());
        let missing = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![],
        };
        assert!(elicitation_response_from_submission(&context, &missing)
            .expect_err("Codex union requires base or custom answer")
            .contains("required"));
        let secret = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![],
                other_text: Some("actual-secret".into()),
            }],
        };
        let (summary, response) =
            elicitation_response_from_submission(&context, &secret).expect("accept secret answer");
        assert_eq!(summary, "Token: ••••••");
        assert!(!summary.contains("actual-secret"));
        assert_eq!(
            serde_json::to_value(response).expect("serialize secret response")["content"]
                ["token__other"],
            "actual-secret"
        );
    }

    #[test]
    fn elicitation_decline_and_cancel_remain_distinct_wire_actions() {
        let request = form_request(
            "Optional note",
            serde_json::json!({
                "type": "object",
                "properties": { "note": { "type": "string" } }
            }),
        );
        let (_, context) = normalized_form(&request).expect("normalize optional form");
        for (outcome, action) in [
            (AcpQuestionnaireOutcome::Declined, "decline"),
            (AcpQuestionnaireOutcome::Cancelled, "cancel"),
        ] {
            let (_, response) = elicitation_response_from_submission(
                &context,
                &AcpQuestionnaireSubmission {
                    outcome,
                    answers: vec![],
                },
            )
            .expect("terminal form response");
            assert_eq!(
                serde_json::to_value(response).expect("serialize form response"),
                serde_json::json!({ "action": action })
            );
        }
    }

    #[test]
    fn elicitation_rejects_invalid_constraints_and_oversized_schemas() {
        let invalid_pattern = form_request(
            "Value",
            serde_json::json!({
                "type": "object",
                "properties": { "value": { "type": "string", "pattern": "[" } }
            }),
        );
        assert!(normalized_form(&invalid_pattern).is_err());

        let email = form_request(
            "Email",
            serde_json::json!({
                "type": "object",
                "properties": { "email": { "type": "string", "format": "email" } },
                "required": ["email"]
            }),
        );
        let (_, context) = normalized_form(&email).expect("supported email format");
        let invalid_email = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![],
                other_text: Some("not-an-email".into()),
            }],
        };
        assert!(elicitation_response_from_submission(&context, &invalid_email).is_err());

        let mut schema = ElicitationSchema::new();
        for index in 0..=MAX_ELICITATION_PROPERTIES {
            schema.properties.insert(
                format!("property_{index}"),
                ElicitationPropertySchema::String(StringPropertySchema::new()),
            );
        }
        assert!(elicitation_form_context(&schema).is_err());
    }

    #[test]
    fn qwen_answers_use_question_indexes_and_join_multi_select_values() {
        let context = QwenQuestionnaireContext {
            questions: vec![
                QwenQuestion {
                    header: "Language".into(),
                    question: "Language?".into(),
                    multi_select: false,
                    options: vec![QwenQuestionOption {
                        label: "TypeScript".into(),
                        description: None,
                    }],
                },
                QwenQuestion {
                    header: "Checks".into(),
                    question: "Checks?".into(),
                    multi_select: true,
                    options: vec![QwenQuestionOption {
                        label: "Unit tests".into(),
                        description: None,
                    }],
                },
            ],
            selected_option_id: "proceed_once".into(),
        };
        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![
                AcpQuestionnaireAnswer {
                    question_index: 0,
                    selected_option_indexes: vec![0],
                    other_text: None,
                },
                AcpQuestionnaireAnswer {
                    question_index: 1,
                    selected_option_indexes: vec![0],
                    other_text: Some("Security scan".into()),
                },
            ],
        };
        let (_, response) =
            qwen_response_from_submission(&context, &submission).expect("valid Qwen response");
        assert_eq!(
            serde_json::to_value(response).expect("serialize Qwen response"),
            serde_json::json!({
                "outcome": { "outcome": "selected", "optionId": "proceed_once" },
                "answers": { "0": "TypeScript", "1": "Unit tests, Security scan" }
            })
        );
    }

    #[test]
    fn standard_plan_classifier_requires_verified_metadata_or_switch_mode() {
        let codex = serde_json::json!({
            "toolCall": { "kind": "think", "rawInput": { "plan": "Codex plan" } },
            "_meta": { "codex": { "kind": "plan_review" } }
        });
        let claude = serde_json::json!({
            "toolCall": { "kind": "switch_mode", "rawInput": { "plan": "Claude plan" } }
        });
        let ordinary = serde_json::json!({
            "toolCall": { "kind": "execute", "rawInput": { "plan": "not a review" } }
        });
        assert_eq!(standard_plan_review(&codex), Some("Codex plan"));
        assert_eq!(standard_plan_review(&claude), Some("Claude plan"));
        assert_eq!(standard_plan_review(&ordinary), None);
        let normalized_codex = normalized_standard_plan_review(codex, "Codex plan");
        assert_eq!(normalized_codex["supportsFeedback"], true);
        assert_eq!(normalized_codex["feedbackDelivery"], "follow_up_prompt");
        assert_eq!(
            normalized_standard_plan_review(claude, "Claude plan")["supportsFeedback"],
            false
        );
    }

    #[test]
    fn request_permission_without_tool_call_is_preserved_for_manual_review() {
        let wire = serde_json::json!({
            "sessionId": "session-autohand",
            "options": [
                { "optionId": "run", "name": "Run", "kind": "allow_once" },
                { "optionId": "cancel", "name": "Cancel", "kind": "reject_once" }
            ],
            "_meta": {
                "title": "Choose execution mode",
                "prompt": "Select how Autohand should continue",
                "description": "This choice is not a tool execution",
                "tool": "mode_picker"
            }
        });
        let parsed: ExtendedRequestPermissionRequest =
            serde_json::from_value(wire.clone()).expect("off-spec but unambiguous picker parses");
        assert!(parsed.tool_call.is_none());
        assert_eq!(
            permission_request_title(&parsed, &wire).as_deref(),
            Some("Choose execution mode")
        );
        assert_eq!(
            parsed.meta.as_ref().and_then(|meta| meta.get("prompt")),
            wire.pointer("/_meta/prompt")
        );
        assert!(!should_auto_approve_permission(true, &parsed, false));
        assert_eq!(
            serde_json::to_value(&parsed).expect("re-serialize picker"),
            wire
        );
        let normalized = normalized_generic_permission_raw(wire, &parsed);
        assert_eq!(normalized["prompt"], "Select how Autohand should continue");
        assert_eq!(
            normalized["description"],
            "This choice is not a tool execution"
        );
    }

    #[test]
    fn automatic_permission_never_selects_persistent_allow_always() {
        let allow_always = PermissionOption::new(
            "allow-always",
            "Always allow",
            PermissionOptionKind::AllowAlways,
        );
        let allow_once =
            PermissionOption::new("allow-once", "Allow once", PermissionOptionKind::AllowOnce);
        assert_eq!(
            automatic_permission_option_id(&[allow_always.clone(), allow_once]),
            Some("allow-once".into())
        );
        assert_eq!(automatic_permission_option_id(&[allow_always]), None);
    }

    #[test]
    fn qwen_questionnaire_submit_never_selects_allow_always() {
        let request = |options: serde_json::Value| {
            serde_json::from_value::<ExtendedRequestPermissionRequest>(serde_json::json!({
                "sessionId": "session-qwen",
                "toolCall": {
                    "toolCallId": "question-qwen",
                    "kind": "think",
                    "_meta": {
                        "qwenInteractionKind": "user_question",
                        "qwenQuestions": [{
                            "header": "Language",
                            "question": "Which language?",
                            "options": [{ "label": "Rust" }]
                        }]
                    }
                },
                "options": options
            }))
            .expect("parse Qwen permission extension")
        };
        let mixed = request(serde_json::json!([
            { "optionId": "persist", "name": "Always", "kind": "allow_always" },
            { "optionId": "submit", "name": "Submit", "kind": "allow_once" }
        ]));
        assert_eq!(
            qwen_questionnaire_context(&mixed)
                .expect("valid Qwen questionnaire")
                .expect("Qwen questionnaire context")
                .selected_option_id,
            "submit"
        );
        let persistent_only = request(serde_json::json!([
            { "optionId": "persist", "name": "Always", "kind": "allow_always" }
        ]));
        assert!(qwen_questionnaire_context(&persistent_only).is_err());
    }

    #[test]
    fn extended_permission_response_keeps_standard_wire_and_options_are_unique() {
        assert_eq!(
            serde_json::to_value(ExtendedRequestPermissionResponse::selected("run"))
                .expect("serialize selected permission"),
            serde_json::json!({
                "outcome": { "outcome": "selected", "optionId": "run" }
            })
        );
        assert_eq!(
            serde_json::to_value(ExtendedRequestPermissionResponse::cancelled())
                .expect("serialize cancelled permission"),
            serde_json::json!({ "outcome": { "outcome": "cancelled" } })
        );
        let duplicate: ExtendedRequestPermissionRequest =
            serde_json::from_value(serde_json::json!({
                "sessionId": "session-1",
                "options": [
                    { "optionId": "same", "name": "One", "kind": "allow_once" },
                    { "optionId": "same", "name": "Two", "kind": "reject_once" }
                ]
            }))
            .expect("parse duplicate option request");
        assert!(validate_permission_options(&duplicate.options)
            .expect_err("duplicate option ids must be rejected")
            .contains("duplicate"));
    }

    fn pending_permission(
        scope: &str,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> (PendingPermission, oneshot::Receiver<PermissionResolution>) {
        let (sender, receiver) = oneshot::channel();
        (
            PendingPermission {
                scope: scope.into(),
                interaction_kind: AcpInteractionKind::Permission,
                tool_call_id: Some("tool-1".into()),
                options: vec![PermissionOptionView {
                    option_id: "allow-once".into(),
                    name: "Allow once".into(),
                    kind: Some("AllowOnce".into()),
                    description: None,
                }],
                questionnaire: None,
                event_tx,
                sender: Some(sender),
            },
            receiver,
        )
    }

    fn pending_questionnaire(
        scope: &str,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> (
        PendingPermission,
        oneshot::Receiver<AcpQuestionnaireSubmission>,
    ) {
        let (sender, receiver) = oneshot::channel();
        (
            PendingPermission {
                scope: scope.into(),
                interaction_kind: AcpInteractionKind::Question,
                tool_call_id: Some("question-tool-1".into()),
                options: vec![],
                questionnaire: Some(PendingQuestionnaire::Grok {
                    context: GrokQuestionnaireContext {
                        questions: vec![GrokQuestion {
                            question: "Which layers?".into(),
                            multi_select: true,
                            options: vec![GrokQuestionOption {
                                label: "Frontend".into(),
                                description: None,
                                preview: None,
                                id: Some("ui".into()),
                            }],
                            id: Some("layers".into()),
                        }],
                        mode: GrokAskUserMode::Default,
                    },
                    sender: Some(sender),
                }),
                event_tx,
                sender: None,
            },
            receiver,
        )
    }

    #[tokio::test]
    async fn resolving_permission_emits_one_selected_terminal_event() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, selected_rx) = pending_permission("scope-1", event_tx);
        runtime
            .permissions
            .lock()
            .await
            .insert("request-1".into(), pending);

        assert!(
            runtime
                .resolve_permission("request-1", "allow-once".into(), None)
                .await
        );
        let resolution = selected_rx.await.expect("selected option");
        assert_eq!(resolution.option_id, "allow-once");
        assert_eq!(resolution.feedback, None);
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                interaction_kind: AcpInteractionKind::Permission,
                tool_call_id: Some(tool_call_id),
                outcome: AcpInteractionOutcome::Selected,
                selected_option_id: Some(option_id),
                selected_option_kind: Some(option_kind),
                selected_option_name: Some(option_name),
            }) if request_id == "request-1"
                && tool_call_id == "tool-1"
                && option_id == "allow-once"
                && option_kind == "AllowOnce"
                && option_name == "Allow once"
        ));
        assert!(event_rx.try_recv().is_err());
        assert!(
            !runtime
                .resolve_permission("request-1", "allow-once".into(), None)
                .await
        );
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn resolving_questionnaire_emits_one_selected_terminal_event() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, response_rx) = pending_questionnaire("scope-1", event_tx);
        runtime
            .permissions
            .lock()
            .await
            .insert("questionnaire-1".into(), pending);
        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![0],
                other_text: None,
            }],
        };

        let summary = runtime
            .resolve_questionnaire("questionnaire-1", submission.clone())
            .await
            .expect("resolve questionnaire");

        assert_eq!(summary, "Which layers?: Frontend");
        assert_eq!(
            response_rx.await.expect("questionnaire response"),
            submission
        );
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                interaction_kind: AcpInteractionKind::Question,
                tool_call_id: Some(tool_call_id),
                outcome: AcpInteractionOutcome::Selected,
                selected_option_id: Some(option_id),
                selected_option_name: Some(option_name),
                ..
            }) if request_id == "questionnaire-1"
                && tool_call_id == "question-tool-1"
                && option_id == "accepted"
                && option_name == "Which layers?: Frontend"
        ));
        assert!(event_rx.try_recv().is_err());
        assert!(runtime
            .resolve_questionnaire("questionnaire-1", submission)
            .await
            .is_err());
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn resolving_empty_plan_questionnaire_preserves_the_selected_action() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (mut pending, response_rx) = pending_questionnaire("scope-1", event_tx);
        pending.interaction_kind = AcpInteractionKind::PlanReview;
        let Some(PendingQuestionnaire::Grok { context, .. }) = pending.questionnaire.as_mut()
        else {
            panic!("Grok questionnaire context");
        };
        context.mode = GrokAskUserMode::Plan;
        runtime
            .permissions
            .lock()
            .await
            .insert("questionnaire-1".into(), pending);
        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::SkipInterview,
            answers: vec![],
        };

        let summary = runtime
            .resolve_questionnaire("questionnaire-1", submission.clone())
            .await
            .expect("resolve empty plan questionnaire");

        assert!(summary.is_empty());
        assert_eq!(
            response_rx.await.expect("questionnaire response"),
            submission
        );
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                interaction_kind: AcpInteractionKind::PlanReview,
                outcome: AcpInteractionOutcome::Selected,
                selected_option_id: Some(option_id),
                selected_option_name: Some(option_name),
                ..
            }) if option_id == "skip_interview" && option_name.is_empty()
        ));
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancelling_scope_emits_one_cancelled_event_only_for_that_scope() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (first, _first_rx) = pending_permission("scope-1", event_tx.clone());
        let (second, _second_rx) = pending_permission("scope-2", event_tx);
        let mut permissions = runtime.permissions.lock().await;
        permissions.insert("request-1".into(), first);
        permissions.insert("request-2".into(), second);
        drop(permissions);

        runtime.cancel_permissions("scope-1").await;

        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                outcome: AcpInteractionOutcome::Cancelled,
                selected_option_id: None,
                ..
            }) if request_id == "request-1"
        ));
        assert!(event_rx.try_recv().is_err());
        assert!(runtime.permissions.lock().await.contains_key("request-2"));
        runtime.cancel_permissions("scope-1").await;
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn expiring_permission_emits_one_expired_terminal_event() {
        let permissions = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, _selected_rx) = pending_permission("scope-1", event_tx);
        permissions.lock().await.insert("request-1".into(), pending);

        expire_permission(&permissions, "request-1").await;
        expire_permission(&permissions, "request-1").await;

        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                outcome: AcpInteractionOutcome::Expired,
                selected_option_id: None,
                ..
            }) if request_id == "request-1"
        ));
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn timeout_wins_a_resolution_race_without_losing_the_terminal_event() {
        let runtime = AcpRuntime::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, selected_rx) = pending_permission("scope-1", event_tx);
        drop(selected_rx);
        runtime
            .permissions
            .lock()
            .await
            .insert("request-1".into(), pending);

        assert!(
            !runtime
                .resolve_permission("request-1", "allow-once".into(), None)
                .await
        );
        expire_permission(&runtime.permissions, "request-1").await;

        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                outcome: AcpInteractionOutcome::Expired,
                ..
            })
        ));
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn interaction_closed_serializes_without_raw_payload_inference() {
        let value = serde_json::to_value(AcpEvent::InteractionClosed {
            request_id: "request-1".into(),
            interaction_kind: AcpInteractionKind::Permission,
            tool_call_id: Some("tool-1".into()),
            outcome: AcpInteractionOutcome::Selected,
            selected_option_id: Some("allow-once".into()),
            selected_option_kind: Some("AllowOnce".into()),
            selected_option_name: Some("Allow once".into()),
        })
        .expect("serialize terminal event");

        assert_eq!(value["type"], "interactionClosed");
        assert_eq!(value["interactionKind"], "permission");
        assert_eq!(value["outcome"], "selected");
        assert_eq!(value["selectedOptionId"], "allow-once");
        assert!(value.get("raw").is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn login_shell_path_reaches_a_bare_acp_process_command() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            std::env::temp_dir().join(format!("aqbot-acp-shell-path-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create fake Agent bin directory");
        let command = format!("aqbot-path-agent-{}", uuid::Uuid::new_v4());
        let executable = directory.join(&command);
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("write fake Agent");
        let mut permissions = std::fs::metadata(&executable)
            .expect("read fake Agent permissions")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("make fake Agent executable");
        let agent = ConfiguredAgent {
            id: "path-agent".into(),
            name: "PATH Agent".into(),
            enabled: true,
            source: "custom".into(),
            command,
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let process_agent =
            configured_agent_for_process_with_path(&agent, directory.to_string_lossy().as_ref());

        let (_, _, _, mut child) = build_acp_agent(&process_agent)
            .spawn_process()
            .expect("login-shell PATH must resolve the bare Agent command");
        let status = child.status().await.expect("wait for fake Agent");

        std::fs::remove_dir_all(&directory).expect("remove fake Agent directory");
        assert!(status.success());
        assert!(agent.env.is_empty(), "runtime PATH must not be persisted");
    }

    #[test]
    fn structured_dependency_errors_are_sanitized_without_unwrapping_business_data() {
        let raw = concat!(
            "Internal error: ",
            r#"{"spawned_at":"/Users/runner/.cargo/registry/src/agent-client-protocol/src/jsonrpc.rs:1732:39","data":{"kind":"spawn","data":"missing runtime"}}"#
        );

        let error = summarize_agent_spawn_error(raw, "npx");

        assert!(
            error.contains(r#""kind":"spawn""#),
            "missing structured data: {error}"
        );
        assert!(
            error.contains(r#""data":"missing runtime""#),
            "ordinary data field was unwrapped: {error}"
        );
        assert!(
            !error.contains("spawned_at")
                && !error.contains("/Users/runner")
                && !error.contains("jsonrpc.rs"),
            "dependency build path leaked into the user-facing error: {error}"
        );
    }

    #[test]
    fn null_dependency_error_data_does_not_leak_its_spawn_location() {
        let raw = concat!(
            "Internal error: ",
            r#"{"spawned_at":"/Users/runner/.cargo/registry/src/agent-client-protocol/src/jsonrpc.rs:1732:39","data":null}"#
        );

        let error = summarize_agent_spawn_error(raw, "npx");

        assert_eq!(error, "null");
        assert!(!error.contains("spawned_at") && !error.contains("/Users/runner"));
    }

    #[test]
    fn ordinary_json_data_is_not_treated_as_a_dependency_wrapper() {
        let raw = r#"Internal error: {"data":"business reason","code":42}"#;

        let error = summarize_agent_spawn_error(raw, "npx");

        assert!(error.contains(r#""data":"business reason""#));
        assert!(error.contains(r#""code":42"#));
    }

    #[tokio::test]
    async fn missing_agent_executable_reports_the_command_without_dependency_source_paths() {
        let runtime = AcpRuntime::new();
        let command = format!("aqbot-missing-acp-agent-{}", uuid::Uuid::new_v4());
        let agent = ConfiguredAgent {
            id: "missing-agent".into(),
            name: "Missing Agent".into(),
            enabled: true,
            source: "custom".into(),
            command: command.clone(),
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };

        let error = runtime
            .prewarm_agent(&agent, false, RuntimeLimits::new(60, 1))
            .await
            .expect_err("a missing ACP executable must fail startup")
            .to_string();

        assert!(error.contains(&command), "missing launch command: {error}");
        assert!(
            error.to_ascii_lowercase().contains("os error 2"),
            "missing operating-system reason: {error}"
        );
        assert!(
            !error.contains("agent-client-protocol") && !error.contains("jsonrpc.rs"),
            "dependency build path leaked into the user-facing error: {error}"
        );
    }

    #[tokio::test]
    async fn cancel_delivery_failure_tears_down_the_inflight_process_scope() {
        let runtime = AcpRuntime::new();
        let limits = RuntimeLimits::new(60, 1);
        let agent = ConfiguredAgent {
            id: "closed-cancel-transport".into(),
            name: "Closed cancel transport".into(),
            enabled: true,
            source: "custom".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "sleep 30".into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let anchor = spawn_process_anchor(&agent, false, limits, runtime.permissions.clone())
            .expect("spawn process anchor");
        let live = spawn_logical_session(
            &anchor,
            &agent,
            std::env::current_dir().expect("current directory"),
            false,
            limits,
            runtime.permissions.clone(),
        );
        live.prompt_generation.store(1, Ordering::Release);
        live.prompt_state.store(PROMPT_RUNNING, Ordering::Release);
        live.active.lock().await.id = Some(SessionId::new("session-1"));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        *live.event_slot.lock().await = Some(event_tx);
        runtime
            .warm_sessions
            .lock()
            .await
            .insert(anchor.fingerprint.clone(), anchor);
        runtime
            .sessions
            .lock()
            .await
            .insert("thread-a".into(), live.clone());

        assert!(
            tokio::time::timeout(Duration::from_secs(2), runtime.cancel("thread-a"))
                .await
                .expect("cancel failure teardown is bounded")
                .expect("cancel reports handled")
        );
        assert!(live.process_shutdown.load(Ordering::Acquire));
        assert!(!runtime.has_live_session("thread-a").await);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(AcpEvent::Status { message })
                if message == ACP_STATUS_CANCEL_RESTARTING
        ));
    }

    #[test]
    fn parses_grok_retry_extension_status() {
        let params = serde_json::value::to_raw_value(&serde_json::json!({
            "session_id": "session-42",
            "update": {
                "session_update": "retry_state",
                "attempt": 3,
                "maxRetries": 15,
                "status": "rate limited"
            }
        }))
        .map(Arc::from)
        .expect("encode extension params");
        let notification = ExtNotification::new("_x.ai/session_notification", params);

        let (session_id, message) = grok_retry_status(&notification).expect("retry status");

        assert_eq!(session_id.to_string(), "session-42");
        assert_eq!(
            message,
            r#"aqbot:grok-retry:{"attempt":3,"maximum":15,"detail":"rate limited"}"#
        );
    }

    fn prompt_attachment(mime_type: &str, data: Option<&str>) -> AcpPromptAttachment {
        AcpPromptAttachment {
            file_name: if is_image_mime_type(mime_type) {
                "diagram.png".into()
            } else {
                "notes.md".into()
            },
            mime_type: mime_type.into(),
            file_size: 42,
            data: data.map(str::to_owned),
            file_uri: if is_image_mime_type(mime_type) {
                "file:///tmp/diagram.png".into()
            } else {
                "file:///tmp/notes.md".into()
            },
        }
    }

    #[test]
    fn builds_text_image_and_resource_link_prompt_blocks() {
        let input = AcpPromptInput {
            text: "Explain these files".into(),
            attachments: vec![
                prompt_attachment("image/png", Some("aW1hZ2U=")),
                prompt_attachment("text/markdown", None),
            ],
        };
        let capabilities =
            AgentCapabilities::new().prompt_capabilities(PromptCapabilities::new().image(true));

        let blocks = prompt_content_blocks(&input, &capabilities).expect("valid prompt blocks");

        assert_eq!(blocks.len(), 3);
        assert!(matches!(
            &blocks[0],
            ContentBlock::Text(content) if content.text == "Explain these files"
        ));
        assert!(matches!(
            &blocks[1],
            ContentBlock::Image(content)
                if content.data == "aW1hZ2U="
                    && content.mime_type == "image/png"
                    && content.uri.as_deref() == Some("file:///tmp/diagram.png")
        ));
        assert!(matches!(
            &blocks[2],
            ContentBlock::ResourceLink(resource)
                if resource.name == "notes.md"
                    && resource.mime_type.as_deref() == Some("text/markdown")
                    && resource.size == Some(42)
                    && resource.uri == "file:///tmp/notes.md"
        ));
    }

    #[test]
    fn resource_links_do_not_require_optional_prompt_capabilities() {
        let input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("application/pdf", None)],
        };

        let blocks = prompt_content_blocks(&input, &AgentCapabilities::default())
            .expect("resource links are an ACP baseline capability");

        assert!(matches!(blocks.as_slice(), [ContentBlock::ResourceLink(_)]));
    }

    #[test]
    fn rejects_images_without_the_advertised_capability_or_payload() {
        let input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("image/png", Some("aW1hZ2U="))],
        };
        let error = prompt_content_blocks(&input, &AgentCapabilities::default())
            .expect_err("image capability is mandatory");
        assert!(error.to_string().contains("image prompt capability"));

        let uppercase_input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("IMAGE/PNG", Some("aW1hZ2U="))],
        };
        let error = prompt_content_blocks(&uppercase_input, &AgentCapabilities::default())
            .expect_err("MIME matching must not bypass image capability");
        assert!(error.to_string().contains("image prompt capability"));

        let disguised_image = AcpPromptInput {
            text: String::new(),
            attachments: vec![AcpPromptAttachment {
                file_name: "diagram.PNG".into(),
                mime_type: "application/x-custom".into(),
                file_size: 42,
                data: Some("aW1hZ2U=".into()),
                file_uri: "file:///tmp/diagram.PNG".into(),
            }],
        };
        let error = prompt_content_blocks(&disguised_image, &AgentCapabilities::default())
            .expect_err("image extensions must not bypass image capability");
        assert!(error.to_string().contains("image prompt capability"));
        let capabilities =
            AgentCapabilities::new().prompt_capabilities(PromptCapabilities::new().image(true));
        let blocks = prompt_content_blocks(&disguised_image, &capabilities)
            .expect("supported image extension is normalized");
        assert!(matches!(
            blocks.as_slice(),
            [ContentBlock::Image(image)] if image.mime_type == "image/png"
        ));

        let input = AcpPromptInput {
            text: String::new(),
            attachments: vec![prompt_attachment("image/png", None)],
        };
        let capabilities =
            AgentCapabilities::new().prompt_capabilities(PromptCapabilities::new().image(true));
        let error =
            prompt_content_blocks(&input, &capabilities).expect_err("image data is mandatory");
        assert!(error.to_string().contains("no Base64 payload"));
    }

    #[test]
    fn rejects_an_empty_prompt_input() {
        let error = prompt_content_blocks(
            &AcpPromptInput {
                text: String::new(),
                attachments: Vec::new(),
            },
            &AgentCapabilities::default(),
        )
        .expect_err("prompt must contain a block");
        assert!(error.to_string().contains("text or an attachment"));
    }

    #[tokio::test]
    async fn prompt_handle_surfaces_a_worker_exit() {
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let permissions = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (pending, _selected_rx) = pending_permission("scope-1", event_tx);
        permissions.lock().await.insert("request-1".into(), pending);
        let (reply_tx, reply_rx) = oneshot::channel();
        drop(reply_tx);
        let handle = AcpPromptHandle {
            session_key: "thread-1".into(),
            permission_scope: "scope-1".into(),
            permissions,
            sessions,
            reply_rx,
        };

        let error = handle.wait().await.expect_err("closed worker must fail");

        assert!(error.to_string().contains("session worker exited"));
        assert!(matches!(
            event_rx.recv().await,
            Some(AcpEvent::InteractionClosed {
                request_id,
                outcome: AcpInteractionOutcome::Cancelled,
                ..
            }) if request_id == "request-1"
        ));
    }

    #[test]
    fn extended_new_session_request_keeps_required_standard_fields() {
        let request = ExtendedNewSessionRequest::new(PathBuf::from("/tmp/project"));
        let serialized = serde_json::to_value(request).expect("serialize session/new request");
        assert_eq!(
            serialized.get("cwd").and_then(|value| value.as_str()),
            Some("/tmp/project")
        );
        assert_eq!(serialized.get("mcpServers"), Some(&serde_json::json!([])));
    }

    #[test]
    fn initialize_request_identifies_aqbot_and_its_supported_capabilities() {
        let serialized =
            serde_json::to_value(aqbot_initialize_request()).expect("serialize initialize request");

        assert_eq!(
            serialized,
            serde_json::json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": {
                        "readTextFile": false,
                        "writeTextFile": false
                    },
                    "terminal": false,
                    "elicitation": {
                        "form": {}
                    },
                    "session": {
                        "configOptions": {
                            "boolean": {}
                        }
                    }
                },
                "clientInfo": {
                    "name": "aqbot",
                    "title": "AQBot",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })
        );
    }

    #[tokio::test]
    async fn prepare_rejects_an_agent_that_negotiates_protocol_version_two() {
        const AGENT: &str = r#"
import json
import sys

def respond(request_id, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    if message.get("method") == "initialize":
        respond(message["id"], {"protocolVersion": 2, "agentCapabilities": {}})
    elif message.get("method") == "session/new":
        respond(message["id"], {"sessionId": "unsupported-version-session"})
"#;
        let runtime = AcpRuntime::new();
        let agent = ConfiguredAgent {
            id: "unsupported-protocol-agent".into(),
            name: "Unsupported protocol agent".into(),
            enabled: true,
            source: "custom".into(),
            command: "python3".into(),
            args: vec!["-u".into(), "-c".into(), AGENT.into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let error = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.prepare(
                "thread-unsupported-version",
                &agent,
                std::env::current_dir().expect("current directory"),
                None,
                false,
                RuntimeLimits::new(60, 1),
                event_tx,
            ),
        )
        .await
        .expect("unsupported-version handshake must finish")
        .expect_err("protocol version 2 must be rejected");

        assert!(
            error
                .to_string()
                .contains("unsupported ACP protocol version 2"),
            "{error}"
        );
        assert!(!runtime.has_live_session("thread-unsupported-version").await);
        while let Ok(event) = event_rx.try_recv() {
            assert!(
                !matches!(event, AcpEvent::Status { message } if message == ACP_STATUS_AGENT_READY),
                "unsupported handshake entered Ready"
            );
        }
    }

    #[tokio::test]
    async fn prepare_times_out_and_drops_the_process_when_session_new_never_responds() {
        const AGENT: &str = r#"
import json
import sys

for line in sys.stdin:
    message = json.loads(line)
    if message.get("method") == "initialize":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {"protocolVersion": 1, "agentCapabilities": {}}
        }), flush=True)
"#;
        let runtime = AcpRuntime::new();
        let agent = ConfiguredAgent {
            id: "hanging-session-new-agent".into(),
            name: "Hanging session/new agent".into(),
            enabled: true,
            source: "custom".into(),
            command: "python3".into(),
            args: vec!["-u".into(), "-c".into(), AGENT.into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let limits =
            RuntimeLimits::new(60, 1).with_session_control_timeout(Duration::from_millis(100));

        let error = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.prepare(
                "thread-hanging-session-new",
                &agent,
                std::env::current_dir().expect("current directory"),
                None,
                false,
                limits,
                event_tx,
            ),
        )
        .await
        .expect("prepare must enforce its session control timeout")
        .expect_err("a hanging session/new request must fail");

        assert!(
            error.to_string().contains("session/new timed out"),
            "{error}"
        );
        assert!(!runtime.has_live_session("thread-hanging-session-new").await);
        assert!(runtime.warm_sessions.lock().await.is_empty());
    }

    #[tokio::test]
    async fn mode_update_times_out_and_drops_the_unresponsive_process() {
        const AGENT: &str = r#"
import json
import sys

def respond(request_id, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    if message.get("method") == "initialize":
        respond(message["id"], {"protocolVersion": 1, "agentCapabilities": {}})
    elif message.get("method") == "session/new":
        respond(message["id"], {
            "sessionId": "hanging-set-mode-session",
            "modes": {
                "currentModeId": "default",
                "availableModes": [
                    {"id": "default", "name": "Agent"},
                    {"id": "plan", "name": "Plan"}
                ]
            }
        })
"#;
        let runtime = AcpRuntime::new();
        let agent = ConfiguredAgent {
            id: "hanging-set-mode-agent".into(),
            name: "Hanging set mode agent".into(),
            enabled: true,
            source: "custom".into(),
            command: "python3".into(),
            args: vec!["-u".into(), "-c".into(), AGENT.into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let limits =
            RuntimeLimits::new(60, 1).with_session_control_timeout(Duration::from_millis(100));
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        runtime
            .prepare(
                "thread-hanging-set-mode",
                &agent,
                std::env::current_dir().expect("current directory"),
                None,
                false,
                limits,
                event_tx,
            )
            .await
            .expect("prepare session with modes");

        let error = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.set_mode("thread-hanging-set-mode", "plan"),
        )
        .await
        .expect("set_mode must enforce its control timeout")
        .expect_err("an unresponsive mode update must fail");

        assert!(
            error.to_string().contains("session/set_mode timed out"),
            "{error}"
        );
        assert!(!runtime.has_live_session("thread-hanging-set-mode").await);
        assert!(runtime.warm_sessions.lock().await.is_empty());
    }

    #[tokio::test]
    async fn close_timeout_tears_down_the_shared_process_without_deadlocking() {
        const AGENT: &str = r#"
import json
import sys

def respond(request_id, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    if message.get("method") == "initialize":
        respond(message["id"], {
            "protocolVersion": 1,
            "agentCapabilities": {"sessionCapabilities": {"close": {}}}
        })
    elif message.get("method") == "session/new":
        respond(message["id"], {"sessionId": "hanging-close-session"})
"#;
        let runtime = AcpRuntime::new();
        let agent = ConfiguredAgent {
            id: "hanging-close-agent".into(),
            name: "Hanging close agent".into(),
            enabled: true,
            source: "custom".into(),
            command: "python3".into(),
            args: vec!["-u".into(), "-c".into(), AGENT.into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let limits =
            RuntimeLimits::new(60, 1).with_session_control_timeout(Duration::from_millis(100));
        runtime
            .prepare(
                "thread-hanging-close",
                &agent,
                std::env::current_dir().expect("current directory"),
                None,
                false,
                limits,
                mpsc::unbounded_channel().0,
            )
            .await
            .expect("prepare hanging close session");

        let error = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.close_session("thread-hanging-close"),
        )
        .await
        .expect("close timeout teardown must not deadlock")
        .expect_err("hanging session/close must fail");

        assert!(error.to_string().contains("session/close timed out"));
        assert!(!runtime.has_live_session("thread-hanging-close").await);
        assert!(runtime.warm_sessions.lock().await.is_empty());
    }

    #[tokio::test]
    async fn opening_session_replays_only_matching_updates_and_cancels_stale_permission() {
        const AGENT: &str = r#"
import json
import sys

log_path = sys.argv[1]
session_number = 0
first_session_id = None

def send(message):
    print(json.dumps(message), flush=True)

def respond(request_id, result):
    send({"jsonrpc": "2.0", "id": request_id, "result": result})

def update(session_id, text):
    send({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text}
            }
        }
    })

for line in sys.stdin:
    message = json.loads(line)
    if message.get("method") == "initialize":
        respond(message["id"], {"protocolVersion": 1, "agentCapabilities": {}})
    elif message.get("method") == "session/new":
        session_number += 1
        session_id = f"session-{session_number}"
        if session_number == 1:
            first_session_id = session_id
            respond(message["id"], {"sessionId": session_id})
            continue

        update(first_session_id, "stale-a-text")
        update(session_id, "early-b-text")
        permission_id = 7001
        send({
            "jsonrpc": "2.0",
            "id": permission_id,
            "method": "session/request_permission",
            "params": {
                "sessionId": first_session_id,
                "toolCall": {"toolCallId": "stale-a-tool", "title": "Stale A edit"},
                "options": [
                    {"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}
                ]
            }
        })
        while True:
            permission_response = json.loads(sys.stdin.readline())
            if permission_response.get("id") == permission_id:
                outcome = ((permission_response.get("result") or {}).get("outcome") or {}).get("outcome")
                with open(log_path, "w", encoding="utf-8") as log:
                    log.write(outcome or "missing")
                break
        respond(message["id"], {"sessionId": session_id})
"#;
        let log_path = std::env::temp_dir().join(format!(
            "aqbot-acp-stale-opening-permission-{}",
            uuid::Uuid::new_v4()
        ));
        let runtime = AcpRuntime::new();
        let agent = ConfiguredAgent {
            id: "stale-opening-route-agent".into(),
            name: "Stale opening route agent".into(),
            enabled: true,
            source: "custom".into(),
            command: "python3".into(),
            args: vec![
                "-u".into(),
                "-c".into(),
                AGENT.into(),
                log_path.to_string_lossy().into_owned(),
            ],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let limits = RuntimeLimits::new(60, 1);
        let cwd = std::env::current_dir().expect("current directory");
        runtime
            .prepare(
                "thread-a",
                &agent,
                cwd.clone(),
                None,
                false,
                limits,
                mpsc::unbounded_channel().0,
            )
            .await
            .expect("prepare first logical session");
        runtime.drop_session("thread-a").await;

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let snapshot = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.prepare("thread-b", &agent, cwd, None, false, limits, event_tx),
        )
        .await
        .expect("stale permission must be cancelled without blocking session/new")
        .expect("prepare second logical session");

        assert_eq!(snapshot.session_id, "session-2");
        let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
        let text = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::StreamText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text, ["early-b-text"]);
        assert!(events.iter().all(|event| !matches!(
            event,
            AcpEvent::PermissionRequest { .. } | AcpEvent::ToolCall { .. }
        )));
        assert_eq!(
            std::fs::read_to_string(&log_path).expect("read permission outcome"),
            "cancelled"
        );
        std::fs::remove_file(log_path).expect("remove permission outcome log");
    }

    #[test]
    fn extended_new_session_response_skips_future_config_kinds_and_keeps_extensions() {
        let response: ExtendedNewSessionResponse = serde_json::from_value(serde_json::json!({
            "sessionId": "session-forward-compatible",
            "configOptions": [
                {
                    "id": "mode",
                    "name": "Mode",
                    "category": "mode",
                    "type": "select",
                    "currentValue": "default",
                    "options": [{ "value": "default", "name": "Default" }]
                },
                {
                    "id": "future-control",
                    "name": "Future control",
                    "type": "not-yet-supported-by-this-client",
                    "currentValue": { "level": 2 }
                }
            ],
            "models": {
                "currentModelId": "model-a",
                "availableModels": [{ "modelId": "model-a" }]
            },
            "reasoningEfforts": [{ "id": "high", "label": "High" }],
            "_meta": { "vendor/session": true }
        }))
        .expect("a future config kind must not reject session/new");

        let config_options = response
            .standard
            .config_options
            .as_ref()
            .expect("valid config option remains available");
        assert_eq!(config_options.len(), 1);
        assert_eq!(config_options[0].id.to_string(), "mode");
        assert_eq!(
            response
                .models
                .as_ref()
                .and_then(|models| models.get("currentModelId"))
                .and_then(serde_json::Value::as_str),
            Some("model-a")
        );
        assert_eq!(
            response.reasoning_efforts,
            Some(serde_json::json!([{ "id": "high", "label": "High" }]))
        );
        assert_eq!(
            response
                .standard
                .meta
                .as_ref()
                .and_then(|meta| meta.get("vendor/session")),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn user_message_echo_is_never_rendered_as_assistant_output() {
        assert!(is_assistant_message_update("agent_message_chunk"));
        assert!(!is_assistant_message_update("user_message_chunk"));
    }

    #[test]
    fn retries_only_explicit_missing_session_errors() {
        assert!(is_missing_session_error("Session not found"));
        assert!(is_missing_session_error("code=session_not_found"));
        assert!(is_missing_session_error(
            "Resource not found: Session 01abc not found: {uri: Session 01abc not found}"
        ));
        assert!(!is_missing_session_error(
            "session/prompt failed: session rate limit exceeded"
        ));
        assert!(!is_missing_session_error("session database unavailable"));
        assert!(!is_missing_session_error("connection closed"));
    }

    #[test]
    fn maps_legacy_model_state_to_a_live_selector() {
        let state = serde_json::json!({
            "currentModelId": "grok-4.5",
            "availableModels": [
                { "modelId": "grok-4.5", "name": "Grok 4.5" },
                { "modelId": "grok-code", "name": "Grok Code" }
            ]
        });
        let option = legacy_model_option_from_state(&state).expect("model selector");
        assert_eq!(option.category, Some(SessionConfigOptionCategory::Model));
        assert_eq!(option.id.to_string(), "model");
        assert_eq!(
            option
                .meta
                .as_ref()
                .and_then(|meta| meta.get("aqbotSetMethod")),
            Some(&serde_json::Value::String("session/set_model".into()))
        );
        let SessionConfigKind::Select(select) = option.kind else {
            panic!("expected select option");
        };
        assert_eq!(select.current_value.to_string(), "grok-4.5");
        let SessionConfigSelectOptions::Ungrouped(choices) = select.options else {
            panic!("expected flat model choices");
        };
        assert_eq!(choices.len(), 2);
    }

    #[test]
    fn maps_grok_model_metadata_to_live_reasoning_selector() {
        let state = serde_json::json!({
            "currentModelId": "grok-4.5",
            "availableModels": [{
                "modelId": "grok-4.5",
                "name": "Grok 4.5",
                "_meta": {
                    "reasoningEffort": "high",
                    "reasoningEfforts": [
                        { "id": "high", "value": "high", "label": "High Effort", "default": true },
                        { "id": "medium", "value": "medium", "label": "Medium Effort", "default": false }
                    ]
                }
            }]
        });
        let option = legacy_reasoning_option_from_state(&state).expect("reasoning selector");
        assert_eq!(
            option.category,
            Some(SessionConfigOptionCategory::ThoughtLevel)
        );
        assert_eq!(option.id.to_string(), "reasoning_effort");
        assert_eq!(
            option
                .meta
                .as_ref()
                .and_then(|meta| meta.get("aqbotSetMethod")),
            Some(&serde_json::Value::String(
                "session/set_model_reasoning".into()
            ))
        );
        let SessionConfigKind::Select(select) = option.kind else {
            panic!("expected select option");
        };
        assert_eq!(select.current_value.to_string(), "high");
    }

    #[test]
    fn switching_legacy_model_rebuilds_its_reasoning_selector() {
        let mut meta = agent_client_protocol::schema::v1::Meta::new();
        meta.insert(
            "modelState".into(),
            serde_json::json!({
                "currentModelId": "model-a",
                "availableModels": [
                    {
                        "modelId": "model-a",
                        "name": "Model A",
                        "_meta": {
                            "reasoningEffort": "low",
                            "reasoningEfforts": [
                                { "id": "low", "label": "Low" },
                                { "id": "high", "label": "High" }
                            ]
                        }
                    },
                    {
                        "modelId": "model-b",
                        "name": "Model B",
                        "_meta": {
                            "reasoningEffort": "medium",
                            "reasoningEfforts": [
                                { "id": "none", "label": "None" },
                                { "id": "medium", "label": "Medium" }
                            ]
                        }
                    }
                ]
            }),
        );
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: Some(meta),
            launch_config_options: Vec::new(),
        };
        let mut options = normalized_config_options(Vec::new(), &metadata);

        apply_legacy_model_selection(&mut options, metadata.meta.as_ref(), "model-b");

        let model = options
            .iter()
            .find(|option| option.category == Some(SessionConfigOptionCategory::Model))
            .expect("target model selector");
        let SessionConfigKind::Select(model_select) = &model.kind else {
            panic!("expected model select option");
        };
        assert_eq!(model_select.current_value.to_string(), "model-b");
        let reasoning = options
            .iter()
            .find(|option| option.category == Some(SessionConfigOptionCategory::ThoughtLevel))
            .expect("target model reasoning selector");
        let SessionConfigKind::Select(select) = &reasoning.kind else {
            panic!("expected select option");
        };
        assert_eq!(select.current_value.to_string(), "medium");
        let SessionConfigSelectOptions::Ungrouped(choices) = &select.options else {
            panic!("expected flat reasoning choices");
        };
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.value.to_string())
                .collect::<Vec<_>>(),
            ["none", "medium"]
        );
    }

    #[test]
    fn grok_reasoning_update_uses_set_model_metadata_without_restarting() {
        let request = LegacySetModelRequest::with_reasoning(
            SessionId::new("session-1"),
            "grok-4.5",
            "medium",
        );
        assert_eq!(
            serde_json::to_value(request).expect("serialize reasoning update"),
            serde_json::json!({
                "sessionId": "session-1",
                "modelId": "grok-4.5",
                "_meta": { "reasoningEffort": "medium" }
            })
        );
    }

    #[test]
    fn places_grok_reasoning_flag_before_stdio_and_replaces_old_value() {
        let agent = ConfiguredAgent {
            id: "grok-build".into(),
            name: "Grok Build".into(),
            enabled: true,
            source: "registry".into(),
            command: "grok".into(),
            args: vec![
                "agent".into(),
                "--reasoning-effort".into(),
                "low".into(),
                "stdio".into(),
            ],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let updated =
            configured_agent_with_reasoning_effort(&agent, "medium").expect("valid spawn args");
        assert_eq!(
            updated.args,
            ["agent", "--reasoning-effort", "medium", "stdio"]
        );
        assert!(configured_agent_with_reasoning_effort(&agent, "bad value").is_err());
    }

    #[test]
    fn places_copilot_model_before_transport_and_restores_default() {
        let agent = ConfiguredAgent {
            id: "github-copilot-cli".into(),
            name: "GitHub Copilot".into(),
            enabled: true,
            source: "registry".into(),
            command: "npx".into(),
            args: vec![
                "-y".into(),
                "@github/copilot@1.0.78".into(),
                "--model=auto".into(),
                "--acp".into(),
            ],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let selected = configured_agent_with_model(&agent, "gpt-5.6-sol").expect("model args");
        assert_eq!(
            selected.args,
            [
                "-y",
                "@github/copilot@1.0.78",
                "--model",
                "gpt-5.6-sol",
                "--acp"
            ]
        );
        let restored = configured_agent_with_model(&selected, "__agent_default")
            .expect("remove model override");
        assert_eq!(restored.args, ["-y", "@github/copilot@1.0.78", "--acp"]);
    }

    #[test]
    fn parses_copilot_cli_model_and_reasoning_catalogs() {
        let config_help = r#"
          `model`: AI model to use.
            - "claude-sonnet-4.6"
            - "gpt-5.6-sol"

          `contextTier`: context window tier.
        "#;
        let command_help = r#"
          --effort, --reasoning-effort <level> Set effort (choices: "none",
                                               "low", "medium", "high", "max")
        "#;
        assert_eq!(
            parse_copilot_models(config_help),
            ["claude-sonnet-4.6", "gpt-5.6-sol"]
        );
        assert_eq!(
            parse_copilot_reasoning_efforts(command_help),
            ["none", "low", "medium", "high", "max"]
        );
    }

    #[test]
    fn discovered_copilot_models_use_the_live_structured_setter() {
        let option = launch_live_model_option(
            "__agent_default".into(),
            &["auto".into(), "gpt-5.6-sol".into()],
        );
        let meta = option.meta.as_ref().expect("host route metadata");
        assert_eq!(
            meta.get("aqbotSetMethod"),
            Some(&serde_json::Value::String("session/set_model".into()))
        );
        assert!(!meta.contains_key("aqbotSpawnArg"));
        let SessionConfigKind::Select(select) = &option.kind else {
            panic!("expected model selector");
        };
        assert_eq!(select.current_value.to_string(), "auto");
        let SessionConfigSelectOptions::Ungrouped(choices) = &select.options else {
            panic!("expected flat model choices");
        };
        assert!(choices
            .iter()
            .all(|choice| choice.value.to_string() != "__agent_default"));
    }

    #[test]
    fn grok_exit_plan_mode_uses_the_verified_wire_contract() {
        let request: GrokExitPlanModeRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "toolCallId": "call-plan-1",
            "planContent": "## Plan\n1. Inspect\n2. Test"
        }))
        .expect("parse Grok plan review");
        assert_eq!(request.session_id.to_string(), "session-1");
        assert_eq!(request.tool_call_id.as_deref(), Some("call-plan-1"));
        assert_eq!(
            serde_json::to_value(GrokExitPlanModeResponse::new("approved"))
                .expect("serialize plan response"),
            serde_json::json!({ "outcome": "approved" })
        );
    }

    #[test]
    fn grok_questionnaire_preserves_question_option_and_freeform_contract() {
        let request: GrokAskUserRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "toolCallId": "call-ask-1",
            "mode": "plan",
            "questions": [
                {
                    "id": "layers",
                    "question": "Which layers?",
                    "multiSelect": true,
                    "options": [
                        { "id": "ui", "label": "Frontend", "description": "Web UI" },
                        { "id": "api", "label": "Backend", "description": "Rust API" }
                    ]
                },
                {
                    "question": "Which store?",
                    "multiSelect": false,
                    "options": [{
                        "id": "postgres-id",
                        "label": "Postgres",
                        "preview": "CREATE TABLE events (...);"
                    }]
                },
                {
                    "question": "Anything else?",
                    "options": []
                }
            ]
        }))
        .expect("parse Grok question");
        assert_eq!(request.mode, GrokAskUserMode::Plan);
        assert!(request.questions[0].multi_select);
        assert_eq!(request.questions[0].id.as_deref(), Some("layers"));
        assert_eq!(
            request.questions[1].options[0].id.as_deref(),
            Some("postgres-id")
        );

        // Deliberately submit questions and choices out of order. The host must
        // map indexes back to the original Agent-provided order.
        let submission = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![
                AcpQuestionnaireAnswer {
                    question_index: 2,
                    selected_option_indexes: vec![],
                    other_text: Some("  请使用中文  ".into()),
                },
                AcpQuestionnaireAnswer {
                    question_index: 1,
                    selected_option_indexes: vec![0],
                    other_text: None,
                },
                AcpQuestionnaireAnswer {
                    question_index: 0,
                    selected_option_indexes: vec![1, 0],
                    other_text: Some("  Keep mobile unchanged  ".into()),
                },
            ],
        };
        let context = GrokQuestionnaireContext {
            questions: request.questions.clone(),
            mode: request.mode,
        };
        validate_questionnaire_submission(&context, &submission)
            .expect("valid multi-question submission");
        let response = GrokAskUserResponse::from_submission(&request, &submission);
        let GrokAskUserResponse::Accepted {
            answers,
            annotations,
        } = &response
        else {
            panic!("expected accepted response");
        };
        assert_eq!(
            answers.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["Which layers?", "Which store?", "Anything else?"]
        );
        assert_eq!(answers["Which layers?"], ["Frontend", "Backend"]);
        assert_eq!(answers["Which store?"], ["Postgres"]);
        assert_eq!(answers["Anything else?"], ["Other"]);
        let annotations = annotations.as_ref().expect("answer annotations");
        assert_eq!(
            annotations["Which layers?"].notes.as_deref(),
            Some("  Keep mobile unchanged  ")
        );
        assert_eq!(
            annotations["Which store?"].preview.as_deref(),
            Some("CREATE TABLE events (...);")
        );
        assert_eq!(
            annotations["Anything else?"].notes.as_deref(),
            Some("  请使用中文  ")
        );

        let serialized = serde_json::to_string(&response).expect("serialize accepted answer");
        assert!(!serialized.contains("postgres-id"));
        assert!(serialized.find("Which layers?") < serialized.find("Which store?"));
        assert!(serialized.find("Which store?") < serialized.find("Anything else?"));
        assert_eq!(
            serde_json::to_value(response).expect("serialize accepted answer"),
            serde_json::json!({
                "outcome": "accepted",
                "answers": {
                    "Which layers?": ["Frontend", "Backend"],
                    "Which store?": ["Postgres"],
                    "Anything else?": ["Other"]
                },
                "annotations": {
                    "Which layers?": { "notes": "  Keep mobile unchanged  " },
                    "Which store?": { "preview": "CREATE TABLE events (...);" },
                    "Anything else?": { "notes": "  请使用中文  " }
                }
            })
        );
    }

    #[test]
    fn grok_questionnaire_serializes_plan_and_cancel_outcomes_exactly() {
        let request: GrokAskUserRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "mode": "plan",
            "questions": [
                {
                    "question": "Which layers?",
                    "multiSelect": true,
                    "options": [
                        { "label": "Frontend" },
                        { "label": "Backend" }
                    ]
                },
                { "question": "Anything else?", "options": [] }
            ]
        }))
        .expect("parse Grok plan questionnaire");
        let answers = vec![
            AcpQuestionnaireAnswer {
                question_index: 1,
                selected_option_indexes: vec![],
                other_text: Some("notes are intentionally omitted on this wire shape".into()),
            },
            AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![1, 0],
                other_text: None,
            },
        ];

        for (outcome, expected_outcome) in [
            (AcpQuestionnaireOutcome::ChatAboutThis, "chat_about_this"),
            (AcpQuestionnaireOutcome::SkipInterview, "skip_interview"),
        ] {
            let submission = AcpQuestionnaireSubmission {
                outcome,
                answers: answers.clone(),
            };
            let response = GrokAskUserResponse::from_submission(&request, &submission);
            assert_eq!(
                serde_json::to_value(response).expect("serialize plan questionnaire response"),
                serde_json::json!({
                    "outcome": expected_outcome,
                    "partial_answers": {
                        "Which layers?": "Frontend, Backend",
                        "Anything else?": "Other"
                    }
                })
            );
        }

        assert_eq!(
            serde_json::to_value(GrokAskUserResponse::cancelled())
                .expect("serialize cancelled answer"),
            serde_json::json!({ "outcome": "cancelled" })
        );
    }

    #[test]
    fn grok_questionnaire_rejects_invalid_or_plan_only_submissions() {
        let question = GrokQuestion {
            question: "Choose one".into(),
            multi_select: false,
            options: vec![GrokQuestionOption {
                label: "A".into(),
                description: None,
                preview: None,
                id: None,
            }],
            id: None,
        };
        let context = GrokQuestionnaireContext {
            questions: vec![question],
            mode: GrokAskUserMode::Default,
        };
        let plan_action = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::ChatAboutThis,
            answers: vec![],
        };
        assert!(validate_questionnaire_submission(&context, &plan_action)
            .expect_err("default mode must reject plan action")
            .contains("outside plan mode"));

        let ambiguous_single_choice = AcpQuestionnaireSubmission {
            outcome: AcpQuestionnaireOutcome::Accepted,
            answers: vec![AcpQuestionnaireAnswer {
                question_index: 0,
                selected_option_indexes: vec![0],
                other_text: Some("Other choice".into()),
            }],
        };
        assert!(
            validate_questionnaire_submission(&context, &ambiguous_single_choice)
                .expect_err("single choice cannot include an option and Other")
                .contains("only accepts one answer")
        );
    }

    #[test]
    fn grok_session_selection_overrides_catalog_default_effort() {
        let state = serde_json::json!({
            "currentModelId": "grok-4.5",
            "availableModels": [{
                "modelId": "grok-4.5",
                "_meta": {
                    "reasoningEffort": "high",
                    "reasoningEfforts": [
                        { "value": "high", "label": "High" },
                        { "value": "medium", "label": "Medium" }
                    ]
                }
            }]
        });
        let mut options =
            vec![legacy_reasoning_option_from_state(&state).expect("reasoning selector")];
        let mut meta = serde_json::Map::new();
        meta.insert(
            "x.ai/sessionConfig".into(),
            serde_json::json!({
                "options": [
                    { "id": "high", "category": "mode", "selected": false },
                    { "id": "medium", "category": "mode", "selected": true }
                ]
            }),
        );
        apply_legacy_session_selection(&mut options, Some(&meta));
        let SessionConfigKind::Select(select) = &options[0].kind else {
            panic!("expected select option");
        };
        assert_eq!(select.current_value.to_string(), "medium");
    }

    #[test]
    fn standard_model_config_takes_precedence_over_legacy_metadata() {
        let standard = SessionConfigOption::select(
            "model",
            "Model",
            "standard",
            vec![SessionConfigSelectOption::new("standard", "Standard")],
        )
        .category(SessionConfigOptionCategory::Model);
        let mut meta = serde_json::Map::new();
        meta.insert(
            "modelState".into(),
            serde_json::json!({
                "currentModelId": "legacy",
                "availableModels": [{ "modelId": "legacy" }]
            }),
        );
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: Some(meta),
            launch_config_options: Vec::new(),
        };
        let options = normalized_config_options(vec![standard], &metadata);
        assert_eq!(options.len(), 1);
    }

    #[test]
    fn launch_refresh_preserves_native_same_id_config() {
        let mut native_meta = agent_client_protocol::schema::v1::Meta::new();
        native_meta.insert("vendorNative".into(), serde_json::Value::Bool(true));
        let native = SessionConfigOption::select(
            "model",
            "Native model",
            "native-b",
            vec![
                SessionConfigSelectOption::new("native-a", "Native A"),
                SessionConfigSelectOption::new("native-b", "Native B"),
            ],
        )
        .category(SessionConfigOptionCategory::Model)
        .meta(native_meta);
        let fallback = SessionConfigOption::select(
            "model",
            "CLI fallback",
            "fallback-a",
            vec![SessionConfigSelectOption::new("fallback-a", "Fallback A")],
        )
        .category(SessionConfigOptionCategory::Model);
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: None,
            launch_config_options: vec![fallback],
        };

        let retained = agent_options_for_launch_refresh(std::slice::from_ref(&native));
        let refreshed = normalized_config_options_for_session(retained, &metadata, &[native]);
        let value = serde_json::to_value(&refreshed).expect("serialize refreshed config");

        assert_eq!(refreshed.len(), 1);
        assert_eq!(value[0]["name"], "Native model");
        assert_eq!(value[0]["currentValue"], "native-b");
        assert_eq!(value[0]["_meta"]["vendorNative"], true);
        assert_eq!(value[0]["options"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn agent_permission_config_overrides_global_auto_approval() {
        let permission = SessionConfigOption::select(
            "mode",
            "Permission",
            "read-only",
            vec![SessionConfigSelectOption::new(
                "read-only",
                "Request approval",
            )],
        )
        .category(SessionConfigOptionCategory::Mode);
        let collaboration = SessionConfigOption::select(
            "collaboration_mode",
            "Collaboration",
            "plan",
            vec![SessionConfigSelectOption::new("plan", "Plan")],
        )
        .category(SessionConfigOptionCategory::Mode);
        assert!(has_agent_permission_config(&[permission]));
        assert!(!has_agent_permission_config(&[collaboration]));
    }

    #[test]
    fn claude_mixed_permission_and_plan_mode_overrides_global_auto_approval() {
        let claude_mode = SessionConfigOption::select(
            "mode",
            "Mode",
            "default",
            vec![
                SessionConfigSelectOption::new("default", "Manual"),
                SessionConfigSelectOption::new("acceptEdits", "Accept Edits"),
                SessionConfigSelectOption::new("plan", "Plan Mode"),
                SessionConfigSelectOption::new("bypassPermissions", "Bypass Permissions"),
            ],
        )
        .description("Session permission mode")
        .category(SessionConfigOptionCategory::Mode);

        assert!(has_agent_permission_config(&[claude_mode]));
    }

    #[test]
    fn synthesizes_only_verified_grok_permission_and_plan_controls() {
        let mut meta = serde_json::Map::new();
        meta.insert("grokShell".into(), serde_json::Value::Bool(true));
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: Some(meta),
            launch_config_options: Vec::new(),
        };

        let modes = normalized_session_modes(None, &metadata).expect("Grok session modes");
        assert_eq!(modes.current_mode_id.to_string(), "default");
        assert_eq!(
            modes
                .available_modes
                .iter()
                .map(|mode| mode.id.to_string())
                .collect::<Vec<_>>(),
            ["default", "plan"]
        );

        let options = normalized_config_options(Vec::new(), &metadata);
        let permission = options
            .iter()
            .find(|option| option.id.to_string() == GROK_PERMISSION_CONFIG_ID)
            .expect("Grok permission control");
        assert_eq!(
            permission.category,
            Some(SessionConfigOptionCategory::Other("permissions".into()))
        );
        let SessionConfigKind::Select(select) = &permission.kind else {
            panic!("expected select permission control");
        };
        let SessionConfigSelectOptions::Ungrouped(choices) = &select.options else {
            panic!("expected flat permission choices");
        };
        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.value.to_string())
                .collect::<Vec<_>>(),
            ["default", "auto", "bypassPermissions"]
        );
    }

    #[test]
    fn detects_permission_modes_without_misclassifying_behavior_or_uri_plan_modes() {
        let gemini = SessionModeState::new(
            "default",
            vec![
                SessionMode::new("default", "Default"),
                SessionMode::new("auto_edit", "Auto Edit"),
                SessionMode::new("yolo", "YOLO"),
                SessionMode::new("plan", "Plan"),
            ],
        );
        let behavior = SessionModeState::new(
            "concise",
            vec![
                SessionMode::new("concise", "Concise"),
                SessionMode::new("verbose", "Verbose"),
                SessionMode::new("plan", "Plan"),
            ],
        );
        let copilot = SessionModeState::new(
            "https://agentclientprotocol.com/protocol/session-modes#agent",
            vec![
                SessionMode::new(
                    "https://agentclientprotocol.com/protocol/session-modes#agent",
                    "Agent",
                ),
                SessionMode::new(
                    "https://agentclientprotocol.com/protocol/session-modes#plan",
                    "Plan",
                ),
                SessionMode::new(
                    "https://agentclientprotocol.com/protocol/session-modes#autopilot",
                    "Autopilot",
                ),
            ],
        );

        assert!(has_agent_permission_modes(Some(&gemini)));
        assert!(!has_agent_permission_modes(Some(&behavior)));
        assert!(!has_agent_permission_modes(Some(&copilot)));
    }

    #[test]
    fn native_session_modes_take_precedence_over_grok_adapter_modes() {
        let mut meta = agent_client_protocol::schema::v1::Meta::new();
        meta.insert("grokShell".into(), serde_json::Value::Bool(true));
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: Some(meta),
            launch_config_options: Vec::new(),
        };
        let native = SessionModeState::new("native", vec![SessionMode::new("native", "Native")]);

        assert_eq!(
            normalized_session_modes(Some(native), &metadata)
                .expect("native modes")
                .current_mode_id
                .to_string(),
            "native"
        );
    }

    #[test]
    fn strips_agent_supplied_host_routing_metadata() {
        let mut marker = agent_client_protocol::schema::v1::Meta::new();
        marker.insert(
            "aqbotSpawnArg".into(),
            serde_json::Value::String("--unsafe-agent-controlled-flag".into()),
        );
        marker.insert("vendorHint".into(), serde_json::Value::Bool(true));
        let option = SessionConfigOption::select(
            "vendor-control",
            "Vendor Control",
            "off",
            vec![SessionConfigSelectOption::new("off", "Off")],
        )
        .meta(marker);
        let metadata = AgentMetadata {
            capabilities: AgentCapabilities::default(),
            meta: None,
            launch_config_options: Vec::new(),
        };

        let normalized = normalized_config_options(vec![option], &metadata);
        let meta = normalized[0]
            .meta
            .as_ref()
            .expect("vendor metadata remains");
        assert!(!meta.contains_key("aqbotSpawnArg"));
        assert_eq!(meta.get("vendorHint"), Some(&serde_json::Value::Bool(true)));
    }

    #[test]
    fn separates_copilot_uri_plan_mode_from_permission_config() {
        let plan = SessionConfigOption::select(
            "mode",
            "Mode",
            "https://agentclientprotocol.com/protocol/session-modes#agent",
            vec![
                SessionConfigSelectOption::new(
                    "https://agentclientprotocol.com/protocol/session-modes#agent",
                    "Agent",
                ),
                SessionConfigSelectOption::new(
                    "https://agentclientprotocol.com/protocol/session-modes#plan",
                    "Plan",
                ),
            ],
        )
        .category(SessionConfigOptionCategory::Mode);
        let permission = SessionConfigOption::select(
            "allow_all",
            "Allow All",
            "off",
            vec![
                SessionConfigSelectOption::new("on", "On"),
                SessionConfigSelectOption::new("off", "Off"),
            ],
        )
        .category(SessionConfigOptionCategory::Other("permissions".into()));

        assert!(config_option_contains_plan(&plan));
        assert!(!has_agent_permission_config(&[plan]));
        assert!(has_agent_permission_config(&[permission]));
    }

    #[test]
    fn persists_config_backed_plan_with_its_config_id() {
        let collaboration = SessionConfigOption::select(
            "collaboration_mode",
            "Collaboration",
            "plan",
            vec![
                SessionConfigSelectOption::new("default", "Default"),
                SessionConfigSelectOption::new("plan", "Plan"),
            ],
        )
        .category(SessionConfigOptionCategory::Mode);
        let snapshot = AcpSessionSnapshot {
            session_id: "session-1".into(),
            modes: None,
            config_options: vec![collaboration],
            agent_capabilities: AgentCapabilities::default(),
        };

        let persisted = persisted_mode_id(&snapshot).expect("config plan is persisted");
        assert!(persisted.starts_with(PERSISTED_CONFIG_MODE_PREFIX));
        assert!(persisted.contains("collaboration_mode"));
        assert!(persisted.contains("plan"));
    }
}
