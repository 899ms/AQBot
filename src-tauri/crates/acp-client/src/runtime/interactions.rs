fn elicitation_property_meta(
    schema: &ElicitationPropertySchema,
) -> Option<&agent_client_protocol::schema::v1::Meta> {
    match schema {
        ElicitationPropertySchema::String(value) => value.meta.as_ref(),
        ElicitationPropertySchema::Number(value) => value.meta.as_ref(),
        ElicitationPropertySchema::Integer(value) => value.meta.as_ref(),
        ElicitationPropertySchema::Boolean(value) => value.meta.as_ref(),
        ElicitationPropertySchema::Array(value) => value.meta.as_ref(),
        _ => None,
    }
}

fn elicitation_codex_meta<'a>(
    schema: &'a ElicitationPropertySchema,
    key: &str,
) -> Option<&'a serde_json::Value> {
    elicitation_property_meta(schema)?
        .get("codex")?
        .as_object()?
        .get(key)
}

fn elicitation_property_text(schema: &ElicitationPropertySchema) -> (Option<&str>, Option<&str>) {
    match schema {
        ElicitationPropertySchema::String(value) => {
            (value.title.as_deref(), value.description.as_deref())
        }
        ElicitationPropertySchema::Number(value) => {
            (value.title.as_deref(), value.description.as_deref())
        }
        ElicitationPropertySchema::Integer(value) => {
            (value.title.as_deref(), value.description.as_deref())
        }
        ElicitationPropertySchema::Boolean(value) => {
            (value.title.as_deref(), value.description.as_deref())
        }
        ElicitationPropertySchema::Array(value) => {
            (value.title.as_deref(), value.description.as_deref())
        }
        _ => (None, None),
    }
}

fn elicitation_input_type(schema: &ElicitationPropertySchema, secret: bool) -> &'static str {
    match schema {
        ElicitationPropertySchema::String(_) if secret => "secret",
        ElicitationPropertySchema::String(_) => "text",
        ElicitationPropertySchema::Number(_) => "number",
        ElicitationPropertySchema::Integer(_) => "integer",
        ElicitationPropertySchema::Boolean(_) => "boolean",
        ElicitationPropertySchema::Array(_) => "array",
        _ => "unsupported",
    }
}

fn enum_options(
    values: impl IntoIterator<Item = (String, String, Option<String>)>,
) -> Result<Vec<ElicitationOptionContext>, String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .map(|(value, label, description)| {
            if !seen.insert(value.clone()) {
                return Err(format!("duplicate elicitation option value `{value}`"));
            }
            Ok(ElicitationOptionContext {
                value,
                label,
                description,
            })
        })
        .collect()
}

fn elicitation_property_options(
    schema: &ElicitationPropertySchema,
) -> Result<Vec<ElicitationOptionContext>, String> {
    match schema {
        ElicitationPropertySchema::String(value) => {
            if let Some(options) = value.one_of.as_ref() {
                return enum_options(options.iter().map(|option| {
                    (
                        option.value.clone(),
                        option.title.clone(),
                        option.description.clone(),
                    )
                }));
            }
            enum_options(
                value
                    .enum_values
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|value| (value.clone(), value, None)),
            )
        }
        ElicitationPropertySchema::Boolean(_) => enum_options([
            ("true".into(), "true".into(), None),
            ("false".into(), "false".into(), None),
        ]),
        ElicitationPropertySchema::Array(value) => match &value.items {
            MultiSelectItems::String(items) => enum_options(
                items
                    .values
                    .iter()
                    .cloned()
                    .map(|value| (value.clone(), value, None)),
            ),
            MultiSelectItems::Titled(items) => enum_options(items.options.iter().map(|option| {
                (
                    option.value.clone(),
                    option.title.clone(),
                    option.description.clone(),
                )
            })),
            _ => Err("unsupported elicitation multi-select item schema".into()),
        },
        ElicitationPropertySchema::Number(_) | ElicitationPropertySchema::Integer(_) => Ok(vec![]),
        _ => Err("unsupported elicitation property schema".into()),
    }
}

fn codex_other_answer_target(schema: &ElicitationPropertySchema) -> Option<String> {
    if elicitation_codex_meta(schema, "isOtherAnswer")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return elicitation_codex_meta(schema, "questionId")?
            .as_str()
            .map(str::to_owned);
    }
    let marker = elicitation_property_meta(schema)?
        .get("_askUserQuestionCustomAnswer")?
        .as_object()?;
    if !marker.get("isCustomAnswer")?.as_bool().unwrap_or(false) {
        return None;
    }
    marker.get("questionId")?.as_str().map(str::to_owned)
}

fn normalized_elicitation_question(
    question: &ElicitationQuestionContext,
    request_message: &str,
    single_question: bool,
) -> serde_json::Value {
    let (_, description) = elicitation_property_text(&question.schema);
    let prompt = description.unwrap_or_else(|| {
        if single_question {
            request_message
        } else {
            &question.title
        }
    });
    let mut normalized = serde_json::json!({
        "id": question.id,
        "title": question.title,
        "question": prompt,
        "description": description,
        "required": question.required,
        "inputType": elicitation_input_type(&question.schema, question.secret),
        "secret": question.secret,
        "allowOther": question.other.is_some(),
        "otherPropertyId": question.other.as_ref().map(|other| other.id.as_str()),
        "multiSelect": matches!(question.schema, ElicitationPropertySchema::Array(_)),
        "options": question.options.iter().map(|option| serde_json::json!({
            "label": option.label,
            "description": option.description,
            "value": option.value,
        })).collect::<Vec<_>>(),
    });
    let encoded_schema = serde_json::to_value(&question.schema)
        .expect("elicitation schema from serde is serializable");
    if let Some(object) = normalized.as_object_mut() {
        for key in [
            "format",
            "minLength",
            "maxLength",
            "pattern",
            "minimum",
            "maximum",
            "minItems",
            "maxItems",
        ] {
            if let Some(value) = encoded_schema.get(key) {
                object.insert(key.into(), value.clone());
            }
        }
        if !question.secret {
            if let Some(value) = encoded_schema.get("default") {
                object.insert("default".into(), value.clone());
            }
        }
    }
    normalized
}

const MAX_ELICITATION_PROPERTIES: usize = 64;
const MAX_ELICITATION_OPTIONS: usize = 100;
const MAX_ELICITATION_TEXT_CHARS: usize = 16_384;

fn validate_elicitation_property_contract(
    id: &str,
    property: &ElicitationPropertySchema,
) -> Result<(), String> {
    let (title, description) = elicitation_property_text(property);
    if [title, description]
        .into_iter()
        .flatten()
        .any(|value| value.chars().count() > MAX_ELICITATION_TEXT_CHARS)
    {
        return Err(format!(
            "elicitation property `{id}` contains oversized text"
        ));
    }
    let options = elicitation_property_options(property)?;
    if options.len() > MAX_ELICITATION_OPTIONS {
        return Err(format!("elicitation property `{id}` has too many options"));
    }
    if options.iter().any(|option| {
        option.value.chars().count() > MAX_ELICITATION_TEXT_CHARS
            || option.label.chars().count() > MAX_ELICITATION_TEXT_CHARS
            || option
                .description
                .as_deref()
                .is_some_and(|value| value.chars().count() > MAX_ELICITATION_TEXT_CHARS)
    }) {
        return Err(format!(
            "elicitation property `{id}` contains an oversized option"
        ));
    }
    match property {
        ElicitationPropertySchema::String(schema) => {
            if schema
                .min_length
                .zip(schema.max_length)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            {
                return Err(format!(
                    "elicitation property `{id}` has invalid string bounds"
                ));
            }
            if let Some(pattern) = schema.pattern.as_deref() {
                if pattern.chars().count() > MAX_ELICITATION_TEXT_CHARS {
                    return Err(format!(
                        "elicitation property `{id}` has an oversized pattern"
                    ));
                }
                regex::Regex::new(pattern).map_err(|error| {
                    format!("elicitation property `{id}` has an invalid pattern: {error}")
                })?;
            }
            if let Some(default) = schema.default.as_deref() {
                if default.chars().count() > MAX_ELICITATION_TEXT_CHARS {
                    return Err(format!(
                        "elicitation property `{id}` has an oversized default"
                    ));
                }
                validate_string_elicitation(schema, default)?;
            }
        }
        ElicitationPropertySchema::Number(schema) => {
            if schema
                .minimum
                .zip(schema.maximum)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            {
                return Err(format!(
                    "elicitation property `{id}` has invalid number bounds"
                ));
            }
            if let Some(default) = schema.default {
                elicitation_scalar_value(property, &default.to_string())?;
            }
        }
        ElicitationPropertySchema::Integer(schema) => {
            if schema
                .minimum
                .zip(schema.maximum)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            {
                return Err(format!(
                    "elicitation property `{id}` has invalid integer bounds"
                ));
            }
            if let Some(default) = schema.default {
                elicitation_scalar_value(property, &default.to_string())?;
            }
        }
        ElicitationPropertySchema::Boolean(_) => {}
        ElicitationPropertySchema::Array(schema) => {
            if schema
                .min_items
                .zip(schema.max_items)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            {
                return Err(format!(
                    "elicitation property `{id}` has invalid array bounds"
                ));
            }
            if schema
                .min_items
                .is_some_and(|minimum| minimum as usize > options.len())
            {
                return Err(format!(
                    "elicitation property `{id}` has an impossible minimum"
                ));
            }
            if let Some(default) = schema.default.as_ref() {
                if default.len() > MAX_ELICITATION_OPTIONS {
                    return Err(format!(
                        "elicitation property `{id}` has an oversized default"
                    ));
                }
                let allowed = options
                    .iter()
                    .map(|option| option.value.as_str())
                    .collect::<HashSet<_>>();
                let unique = default.iter().map(String::as_str).collect::<HashSet<_>>();
                if unique.len() != default.len()
                    || default
                        .iter()
                        .any(|value| !allowed.contains(value.as_str()))
                {
                    return Err(format!(
                        "elicitation property `{id}` has an invalid default"
                    ));
                }
                validate_elicitation_array(schema, default.clone())?;
            }
        }
        _ => return Err(format!("unsupported elicitation property `{id}`")),
    }
    Ok(())
}

fn elicitation_form_context(schema: &ElicitationSchema) -> Result<ElicitationFormContext, String> {
    if schema.properties.is_empty() || schema.properties.len() > MAX_ELICITATION_PROPERTIES {
        return Err(format!(
            "elicitation form must contain between 1 and {MAX_ELICITATION_PROPERTIES} properties"
        ));
    }
    if [schema.title.as_deref(), schema.description.as_deref()]
        .into_iter()
        .flatten()
        .any(|value| value.chars().count() > MAX_ELICITATION_TEXT_CHARS)
    {
        return Err("elicitation schema contains oversized text".into());
    }
    let required = schema
        .required
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if schema
        .required
        .as_deref()
        .is_some_and(|required| required.len() != required.iter().collect::<HashSet<_>>().len())
    {
        return Err("elicitation schema contains duplicate required fields".into());
    }
    if required
        .iter()
        .any(|required_id| !schema.properties.contains_key(*required_id))
    {
        return Err("elicitation schema requires an unknown property".into());
    }
    let mut companions = HashMap::new();
    for (id, property) in &schema.properties {
        if id.trim().is_empty() || id.chars().count() > 256 {
            return Err("elicitation property id is invalid".into());
        }
        validate_elicitation_property_contract(id, property)?;
        if let Some(target) = codex_other_answer_target(property) {
            if companions
                .insert(target.clone(), (id.clone(), property.clone()))
                .is_some()
            {
                return Err(format!("duplicate elicitation other field for `{target}`"));
            }
        }
    }

    let mut questions = Vec::new();
    for (id, property) in &schema.properties {
        if codex_other_answer_target(property).is_some() {
            continue;
        }
        if matches!(property, ElicitationPropertySchema::Other(_)) {
            return Err(format!("unsupported elicitation property `{id}`"));
        }
        let (title, _) = elicitation_property_text(property);
        let other = companions
            .remove(id)
            .map(|(id, schema)| ElicitationOtherPropertyContext { id, schema });
        let secret = elicitation_codex_meta(property, "isSecret")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || other.as_ref().is_some_and(|other| {
                elicitation_codex_meta(&other.schema, "isSecret")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            });
        questions.push(ElicitationQuestionContext {
            id: id.clone(),
            title: title.unwrap_or(id).to_string(),
            required: required.contains(id.as_str())
                || other
                    .as_ref()
                    .is_some_and(|other| required.contains(other.id.as_str()))
                || elicitation_codex_meta(property, "isOther")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            secret,
            schema: property.clone(),
            options: elicitation_property_options(property)?,
            other,
        });
    }
    if !companions.is_empty() {
        return Err("elicitation other field references a missing question".into());
    }
    if questions.is_empty() {
        return Err("elicitation form contains no supported questions".into());
    }
    Ok(ElicitationFormContext { questions })
}

fn normalize_elicitation_form(
    request: &CreateElicitationRequest,
    form: &ElicitationFormMode,
) -> Result<(serde_json::Value, ElicitationFormContext), String> {
    if request.message.chars().count() > MAX_ELICITATION_TEXT_CHARS {
        return Err("elicitation message is too large".into());
    }
    let context = elicitation_form_context(&form.requested_schema)?;
    let single_question = context.questions.len() == 1;
    let raw = serde_json::json!({
        "kind": "elicitation_form",
        "message": request.message,
        "questions": context.questions.iter().map(|question| {
            normalized_elicitation_question(question, &request.message, single_question)
        }).collect::<Vec<_>>(),
    });
    Ok((raw, context))
}

async fn handle_elicitation_request(
    request: CreateElicitationRequest,
    responder: Responder<CreateElicitationResponse>,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
) -> Result<(), agent_client_protocol::Error> {
    let prompt_dispatch = prompt_dispatch_lock.lock().await;
    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        return responder.respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
    }
    let ElicitationMode::Form(form) = &request.mode else {
        tracing::warn!("ACP agent requested an unsupported elicitation mode");
        return responder.respond(CreateElicitationResponse::new(ElicitationAction::Decline));
    };
    let Some(event_tx) = event_tx else {
        return responder.respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
    };
    let (raw, context) = match normalize_elicitation_form(&request, form) {
        Ok(normalized) => normalized,
        Err(error) => {
            tracing::warn!(%error, "declining invalid ACP elicitation form");
            return responder.respond(CreateElicitationResponse::new(ElicitationAction::Decline));
        }
    };
    let tool_call_id = match request.scope() {
        ElicitationScope::Session(scope) => scope.tool_call_id.as_ref().map(ToString::to_string),
        _ => None,
    };
    let title = form
        .requested_schema
        .title
        .clone()
        .unwrap_or_else(|| request.message.clone());
    let request_id = uuid::Uuid::new_v4().to_string();
    let options = context
        .questions
        .iter()
        .enumerate()
        .flat_map(|(question_index, question)| {
            question
                .options
                .iter()
                .enumerate()
                .map(move |(option_index, option)| PermissionOptionView {
                    option_id: format!("answer:{question_index}:{option_index}"),
                    name: option.label.clone(),
                    kind: Some("AllowOnce".into()),
                    description: option.description.clone(),
                })
        })
        .collect::<Vec<_>>();
    let (sender, receiver) = oneshot::channel();
    permissions.lock().await.insert(
        request_id.clone(),
        PendingPermission {
            scope: permission_scope,
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id: tool_call_id.clone(),
            options: options.clone(),
            questionnaire: Some(PendingQuestionnaire::Elicitation {
                context,
                sender: Some(sender),
            }),
            event_tx: event_tx.clone(),
            sender: None,
        },
    );
    if event_tx
        .send(AcpEvent::PermissionRequest {
            request_id: request_id.clone(),
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id,
            title: Some(title),
            raw,
            options,
        })
        .is_err()
    {
        permissions.lock().await.remove(&request_id);
        return responder.respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
    }
    drop(prompt_dispatch);

    let response = match tokio::time::timeout(Duration::from_secs(600), receiver).await {
        Ok(Ok(response)) => response,
        _ => {
            expire_permission(&permissions, &request_id).await;
            CreateElicitationResponse::new(ElicitationAction::Cancel)
        }
    };
    responder.respond(response)
}

fn standard_plan_review(raw: &serde_json::Value) -> Option<&str> {
    let plan = raw
        .pointer("/toolCall/rawInput/plan")
        .or_else(|| raw.pointer("/tool_call/raw_input/plan"))?
        .as_str()?
        .trim();
    if plan.is_empty() {
        return None;
    }
    let codex_plan = raw
        .pointer("/_meta/codex/kind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("plan_review"));
    let switch_mode = raw
        .pointer("/toolCall/kind")
        .or_else(|| raw.pointer("/tool_call/kind"))
        .and_then(serde_json::Value::as_str)
        .map(session_mode_token)
        .is_some_and(|kind| kind == "switchmode");
    (codex_plan || switch_mode).then_some(plan)
}

fn is_codex_plan_review(raw: &serde_json::Value) -> bool {
    raw.pointer("/_meta/codex/kind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("plan_review"))
}

fn normalized_standard_plan_review(mut raw: serde_json::Value, plan: &str) -> serde_json::Value {
    let supports_follow_up_feedback = is_codex_plan_review(&raw);
    if let Some(object) = raw.as_object_mut() {
        object.insert(
            "kind".into(),
            serde_json::Value::String("plan_review".into()),
        );
        object.insert("plan".into(), serde_json::Value::String(plan.into()));
        object.insert(
            "supportsFeedback".into(),
            serde_json::Value::Bool(supports_follow_up_feedback),
        );
        if supports_follow_up_feedback {
            object.insert(
                "feedbackDelivery".into(),
                serde_json::Value::String("follow_up_prompt".into()),
            );
        }
    }
    raw
}

fn qwen_questionnaire_context(
    request: &ExtendedRequestPermissionRequest,
) -> Result<Option<QwenQuestionnaireContext>, String> {
    let Some(tool_call) = request.tool_call.as_ref() else {
        return Ok(None);
    };
    let Some(meta) = tool_call.meta.as_ref() else {
        return Ok(None);
    };
    let is_question = meta
        .get("qwenInteractionKind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("user_question"));
    if !is_question {
        return Ok(None);
    }
    let questions_value = meta
        .get("qwenQuestions")
        .cloned()
        .or_else(|| {
            tool_call
                .fields
                .raw_input
                .as_ref()?
                .get("questions")
                .cloned()
        })
        .ok_or_else(|| "Qwen user_question is missing qwenQuestions".to_string())?;
    let questions: Vec<QwenQuestion> = serde_json::from_value(questions_value)
        .map_err(|error| format!("invalid Qwen questionnaire: {error}"))?;
    if questions.is_empty() || questions.len() > 64 {
        return Err("Qwen questionnaire must contain between 1 and 64 questions".into());
    }
    for question in &questions {
        if question.question.trim().is_empty()
            || question.options.len() > MAX_ELICITATION_OPTIONS
            || question.question.chars().count() > MAX_ELICITATION_TEXT_CHARS
            || question.header.chars().count() > MAX_ELICITATION_TEXT_CHARS
            || question.options.iter().any(|option| {
                option.label.chars().count() > MAX_ELICITATION_TEXT_CHARS
                    || option
                        .description
                        .as_deref()
                        .is_some_and(|value| value.chars().count() > MAX_ELICITATION_TEXT_CHARS)
            })
        {
            return Err(
                "Qwen questionnaire contains an invalid question or too many options".into(),
            );
        }
    }
    let selected_option_id = request
        .options
        .iter()
        .find(|option| option.kind == PermissionOptionKind::AllowOnce)
        .map(|option| option.option_id.to_string())
        .ok_or_else(|| "Qwen questionnaire has no submit option".to_string())?;
    Ok(Some(QwenQuestionnaireContext {
        questions,
        selected_option_id,
    }))
}

fn normalized_qwen_questionnaire(context: &QwenQuestionnaireContext) -> serde_json::Value {
    serde_json::json!({
        "kind": "ask_user_question",
        "questions": context.questions.iter().enumerate().map(|(index, question)| {
            serde_json::json!({
                "id": index.to_string(),
                "title": question.header,
                "question": question.question,
                "required": true,
                "inputType": "text",
                "secret": false,
                "allowOther": true,
                "multiSelect": question.multi_select,
                "options": question.options.iter().map(|option| serde_json::json!({
                    "label": option.label,
                    "description": option.description,
                    "value": option.label,
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    })
}

async fn handle_qwen_questionnaire(
    request: &ExtendedRequestPermissionRequest,
    context: QwenQuestionnaireContext,
    responder: Responder<ExtendedRequestPermissionResponse>,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    prompt_dispatch: tokio::sync::MutexGuard<'_, ()>,
) -> Result<(), agent_client_protocol::Error> {
    let tool_call = request
        .tool_call
        .as_ref()
        .expect("Qwen questionnaire classification requires a tool call");
    let request_id = uuid::Uuid::new_v4().to_string();
    let tool_call_id = tool_call.tool_call_id.to_string();
    let raw = normalized_qwen_questionnaire(&context);
    let options = request
        .options
        .iter()
        .map(|option| PermissionOptionView {
            option_id: option.option_id.to_string(),
            name: option.name.clone(),
            kind: Some(format!("{:?}", option.kind)),
            description: None,
        })
        .collect::<Vec<_>>();
    let (sender, receiver) = oneshot::channel();
    permissions.lock().await.insert(
        request_id.clone(),
        PendingPermission {
            scope: permission_scope,
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id: Some(tool_call_id.clone()),
            options: options.clone(),
            questionnaire: Some(PendingQuestionnaire::Qwen {
                context,
                sender: Some(sender),
            }),
            event_tx: event_tx.clone(),
            sender: None,
        },
    );
    if event_tx
        .send(AcpEvent::PermissionRequest {
            request_id: request_id.clone(),
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id: Some(tool_call_id),
            title: tool_call.fields.title.clone(),
            raw,
            options,
        })
        .is_err()
    {
        permissions.lock().await.remove(&request_id);
        return responder.respond(ExtendedRequestPermissionResponse::cancelled());
    }
    drop(prompt_dispatch);
    let response = match tokio::time::timeout(Duration::from_secs(600), receiver).await {
        Ok(Ok(response)) => response,
        _ => {
            expire_permission(&permissions, &request_id).await;
            ExtendedRequestPermissionResponse::cancelled()
        }
    };
    responder.respond(response)
}

fn validate_permission_options(options: &[PermissionOption]) -> Result<(), String> {
    if options.is_empty() || options.len() > MAX_ELICITATION_OPTIONS {
        return Err("permission request must contain between 1 and 100 options".into());
    }
    let mut ids = HashSet::new();
    for option in options {
        let option_id = option.option_id.to_string();
        if option_id.trim().is_empty()
            || option.name.trim().is_empty()
            || option_id.chars().count() > 256
            || option.name.chars().count() > MAX_ELICITATION_TEXT_CHARS
        {
            return Err("permission option id and name must not be empty".into());
        }
        if !ids.insert(option_id.clone()) {
            return Err(format!("duplicate permission option id `{option_id}`"));
        }
        match option.kind {
            PermissionOptionKind::AllowOnce
            | PermissionOptionKind::AllowAlways
            | PermissionOptionKind::RejectOnce
            | PermissionOptionKind::RejectAlways => {}
            _ => {
                return Err(format!(
                    "unsupported permission option kind for `{option_id}`"
                ))
            }
        }
    }
    Ok(())
}

fn validate_permission_metadata(request: &ExtendedRequestPermissionRequest) -> Result<(), String> {
    let Some(meta) = request.meta.as_ref() else {
        return Ok(());
    };
    if ["title", "prompt", "description", "tool"]
        .into_iter()
        .filter_map(|key| meta.get(key)?.as_str())
        .any(|value| value.chars().count() > MAX_ELICITATION_TEXT_CHARS)
    {
        return Err("permission request metadata exceeds the client safety limit".into());
    }
    Ok(())
}

fn permission_request_title(
    request: &ExtendedRequestPermissionRequest,
    raw: &serde_json::Value,
) -> Option<String> {
    request
        .tool_call
        .as_ref()
        .and_then(|tool_call| tool_call.fields.title.clone())
        .or_else(|| {
            let meta = request.meta.as_ref()?;
            ["title", "prompt", "description", "tool"]
                .into_iter()
                .find_map(|key| meta.get(key)?.as_str().map(str::to_owned))
        })
        .or_else(|| {
            raw.get("title")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
}

fn normalized_generic_permission_raw(
    mut raw: serde_json::Value,
    request: &ExtendedRequestPermissionRequest,
) -> serde_json::Value {
    if request.tool_call.is_some() {
        return raw;
    }
    let Some(meta) = request.meta.as_ref() else {
        return raw;
    };
    let Some(object) = raw.as_object_mut() else {
        return raw;
    };
    for key in ["title", "prompt", "description", "tool"] {
        if !object.contains_key(key) {
            if let Some(value) = meta.get(key) {
                object.insert(key.into(), value.clone());
            }
        }
    }
    raw
}

fn should_auto_approve_permission(
    auto: bool,
    request: &ExtendedRequestPermissionRequest,
    is_plan_review: bool,
) -> bool {
    auto && request.tool_call.is_some() && !is_plan_review
}

fn automatic_permission_option_id(options: &[PermissionOption]) -> Option<String> {
    options
        .iter()
        .find(|option| option.kind == PermissionOptionKind::AllowOnce)
        .map(|option| option.option_id.to_string())
}

async fn handle_permission_request(
    request: ExtendedRequestPermissionRequest,
    responder: Responder<ExtendedRequestPermissionResponse>,
    auto: bool,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
) -> Result<(), agent_client_protocol::Error> {
    let prompt_dispatch = prompt_dispatch_lock.lock().await;
    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
        return Ok(());
    }
    if let Err(error) = validate_permission_options(&request.options)
        .and_then(|()| validate_permission_metadata(&request))
    {
        tracing::warn!(%error, "ACP agent sent an invalid permission request");
        responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
        return Ok(());
    }

    let raw = serde_json::to_value(&request).map_err(|error| {
        agent_client_protocol::util::internal_error(format!(
            "failed to serialize permission request: {error}"
        ))
    })?;
    let qwen_context = match qwen_questionnaire_context(&request) {
        Ok(context) => context,
        Err(error) => {
            tracing::warn!(%error, "cancelling invalid Qwen questionnaire");
            responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
            return Ok(());
        }
    };
    if let Some(context) = qwen_context {
        let Some(event_tx) = event_tx else {
            responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
            return Ok(());
        };
        return handle_qwen_questionnaire(
            &request,
            context,
            responder,
            permissions,
            permission_scope,
            event_tx,
            prompt_dispatch,
        )
        .await;
    }
    let plan = standard_plan_review(&raw).map(str::to_owned);

    if should_auto_approve_permission(auto, &request, plan.is_some()) {
        let option_id = automatic_permission_option_id(&request.options);
        if let Some(id) = option_id {
            responder.respond(ExtendedRequestPermissionResponse::selected(id))?;
        } else {
            responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
        }
        return Ok(());
    }

    let Some(event_tx) = event_tx else {
        responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
        return Ok(());
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let options: Vec<PermissionOptionView> = request
        .options
        .iter()
        .map(|o| PermissionOptionView {
            option_id: o.option_id.to_string(),
            name: o.name.clone(),
            kind: Some(format!("{:?}", o.kind)),
            description: None,
        })
        .collect();

    let tool_call_raw = raw
        .get("toolCall")
        .or_else(|| raw.get("tool_call"))
        .cloned();
    let tool_call_id = request
        .tool_call
        .as_ref()
        .map(|tool_call| tool_call.tool_call_id.to_string());
    let tool_kind = tool_call_raw
        .as_ref()
        .and_then(|raw| raw.get("kind"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let tool_status = tool_call_raw
        .as_ref()
        .and_then(|raw| raw.get("status"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let title = permission_request_title(&request, &raw);
    let interaction_kind = if plan.is_some() {
        AcpInteractionKind::PlanReview
    } else {
        AcpInteractionKind::Permission
    };
    let interaction_raw = plan
        .as_deref()
        .map(|plan| normalized_standard_plan_review(raw.clone(), plan))
        .unwrap_or_else(|| normalized_generic_permission_raw(raw, &request));
    let (tx, rx) = oneshot::channel::<PermissionResolution>();
    {
        let mut map = permissions.lock().await;
        map.insert(
            request_id.clone(),
            PendingPermission {
                scope: permission_scope,
                interaction_kind,
                tool_call_id: tool_call_id.clone(),
                options: options.clone(),
                questionnaire: None,
                event_tx: event_tx.clone(),
                sender: Some(tx),
            },
        );
    }
    let tool_event_failed = if interaction_kind == AcpInteractionKind::Permission {
        match (tool_call_id.as_ref(), tool_call_raw) {
            (Some(tool_call_id), Some(tool_call_raw)) => event_tx
                .send(AcpEvent::ToolCall {
                    tool_call_id: tool_call_id.clone(),
                    title: title.clone(),
                    kind: tool_kind,
                    status: tool_status,
                    raw: tool_call_raw,
                })
                .is_err(),
            _ => false,
        }
    } else {
        false
    };
    if tool_event_failed
        || event_tx
            .send(AcpEvent::PermissionRequest {
                request_id: request_id.clone(),
                interaction_kind,
                tool_call_id,
                title,
                raw: interaction_raw,
                options: options.clone(),
            })
            .is_err()
    {
        permissions.lock().await.remove(&request_id);
        responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
        return Ok(());
    }
    drop(prompt_dispatch);

    let selected = tokio::time::timeout(std::time::Duration::from_secs(600), rx).await;
    match selected {
        Ok(Ok(resolution)) => {
            responder.respond(ExtendedRequestPermissionResponse::selected(
                resolution.option_id,
            ))?;
        }
        _ => {
            expire_permission(&permissions, &request_id).await;
            responder.respond(ExtendedRequestPermissionResponse::cancelled())?;
        }
    }
    Ok(())
}

async fn handle_grok_exit_plan_mode(
    request: GrokExitPlanModeRequest,
    responder: Responder<GrokExitPlanModeResponse>,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
) -> Result<(), agent_client_protocol::Error> {
    let prompt_dispatch = prompt_dispatch_lock.lock().await;
    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        responder.respond(GrokExitPlanModeResponse::new("cancelled"))?;
        return Ok(());
    }
    let Some(event_tx) = event_tx else {
        responder.respond(GrokExitPlanModeResponse::new("cancelled"))?;
        return Ok(());
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let options = vec![
        PermissionOptionView {
            option_id: "approved".into(),
            name: "Approve and implement".into(),
            kind: Some("AllowOnce".into()),
            description: None,
        },
        PermissionOptionView {
            option_id: "cancelled".into(),
            name: "Continue planning".into(),
            kind: Some("RejectOnce".into()),
            description: None,
        },
        PermissionOptionView {
            option_id: "abandoned".into(),
            name: "Abandon plan".into(),
            kind: Some("RejectAlways".into()),
            description: None,
        },
    ];
    let mut raw = serde_json::to_value(&request).map_err(|error| {
        agent_client_protocol::util::internal_error(format!(
            "failed to serialize Grok plan review: {error}"
        ))
    })?;
    if let Some(object) = raw.as_object_mut() {
        object.insert(
            "kind".into(),
            serde_json::Value::String("plan_review".into()),
        );
        object.insert(
            "title".into(),
            serde_json::Value::String("Plan review".into()),
        );
        object.insert("supportsFeedback".into(), serde_json::Value::Bool(true));
    }
    let (tx, rx) = oneshot::channel::<PermissionResolution>();
    permissions.lock().await.insert(
        request_id.clone(),
        PendingPermission {
            scope: permission_scope,
            interaction_kind: AcpInteractionKind::PlanReview,
            tool_call_id: request.tool_call_id.clone(),
            options: options.clone(),
            questionnaire: None,
            event_tx: event_tx.clone(),
            sender: Some(tx),
        },
    );
    // Do NOT emit AcpEvent::Plan here — that is reserved for structured
    // session/update plan todos. Plan-review documents would otherwise be
    // mis-parsed as the progress checklist (list lines from planContent).
    if event_tx
        .send(AcpEvent::PermissionRequest {
            request_id: request_id.clone(),
            interaction_kind: AcpInteractionKind::PlanReview,
            tool_call_id: request.tool_call_id.clone(),
            title: None,
            raw,
            options: options.clone(),
        })
        .is_err()
    {
        permissions.lock().await.remove(&request_id);
        responder.respond(GrokExitPlanModeResponse::new("cancelled"))?;
        return Ok(());
    }
    drop(prompt_dispatch);

    let selected = tokio::time::timeout(Duration::from_secs(600), rx).await;
    let resolution = match selected {
        Ok(Ok(resolution))
            if ["approved", "cancelled", "abandoned"].contains(&resolution.option_id.as_str()) =>
        {
            resolution
        }
        _ => {
            expire_permission(&permissions, &request_id).await;
            PermissionResolution {
                option_id: "cancelled".into(),
                feedback: None,
            }
        }
    };
    responder.respond(GrokExitPlanModeResponse {
        outcome: resolution.option_id,
        feedback: resolution.feedback,
    })?;
    Ok(())
}

async fn handle_grok_ask_user(
    request: GrokAskUserRequest,
    responder: Responder<GrokAskUserResponse>,
    permissions: PermissionMap,
    permission_scope: String,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
    prompt_state: Arc<AtomicU8>,
    prompt_dispatch_lock: Arc<Mutex<()>>,
) -> Result<(), agent_client_protocol::Error> {
    let prompt_dispatch = prompt_dispatch_lock.lock().await;
    if prompt_state.load(Ordering::Acquire) == PROMPT_CANCEL_REQUESTED {
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    }
    let Some(event_tx) = event_tx else {
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    };
    let Some(first_question) = request.questions.first() else {
        tracing::warn!("Grok sent an empty ask_user_question questionnaire");
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let options = request
        .questions
        .iter()
        .enumerate()
        .flat_map(|(question_index, question)| {
            question
                .options
                .iter()
                .enumerate()
                .map(move |(option_index, option)| PermissionOptionView {
                    option_id: format!("answer:{question_index}:{option_index}"),
                    name: option.label.clone(),
                    kind: Some("AllowOnce".into()),
                    description: option.description.clone(),
                })
        })
        .collect::<Vec<_>>();
    let mut raw = serde_json::to_value(&request).map_err(|error| {
        agent_client_protocol::util::internal_error(format!(
            "failed to serialize Grok user question: {error}"
        ))
    })?;
    if let Some(object) = raw.as_object_mut() {
        object.insert(
            "kind".into(),
            serde_json::Value::String("ask_user_question".into()),
        );
        object.insert(
            "title".into(),
            serde_json::Value::String(first_question.question.clone()),
        );
    }
    let (tx, rx) = oneshot::channel::<AcpQuestionnaireSubmission>();
    permissions.lock().await.insert(
        request_id.clone(),
        PendingPermission {
            scope: permission_scope,
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id: request.tool_call_id.clone(),
            options: options.clone(),
            questionnaire: Some(PendingQuestionnaire::Grok {
                context: GrokQuestionnaireContext {
                    questions: request.questions.clone(),
                    mode: request.mode,
                },
                sender: Some(tx),
            }),
            event_tx: event_tx.clone(),
            sender: None,
        },
    );
    if event_tx
        .send(AcpEvent::PermissionRequest {
            request_id: request_id.clone(),
            interaction_kind: AcpInteractionKind::Question,
            tool_call_id: request.tool_call_id.clone(),
            title: Some(first_question.question.clone()),
            raw,
            options: options.clone(),
        })
        .is_err()
    {
        permissions.lock().await.remove(&request_id);
        responder.respond(GrokAskUserResponse::cancelled())?;
        return Ok(());
    }
    drop(prompt_dispatch);

    let selected = tokio::time::timeout(Duration::from_secs(600), rx).await;
    let response = match selected {
        Ok(Ok(submission)) => GrokAskUserResponse::from_submission(&request, &submission),
        _ => {
            expire_permission(&permissions, &request_id).await;
            GrokAskUserResponse::cancelled()
        }
    };
    responder.respond(response)?;
    Ok(())
}
