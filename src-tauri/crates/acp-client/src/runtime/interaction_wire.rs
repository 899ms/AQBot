#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "session/set_model", response = LegacySetModelResponse)]
#[serde(rename_all = "camelCase")]
struct LegacySetModelRequest {
    session_id: SessionId,
    model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    meta: Option<agent_client_protocol::schema::v1::Meta>,
}

impl LegacySetModelRequest {
    fn new(session_id: SessionId, model_id: &str) -> Self {
        Self {
            session_id,
            model_id: model_id.to_string(),
            meta: None,
        }
    }

    fn with_reasoning(session_id: SessionId, model_id: &str, reasoning_effort: &str) -> Self {
        let mut meta = agent_client_protocol::schema::v1::Meta::new();
        meta.insert(
            "reasoningEffort".into(),
            serde_json::Value::String(reasoning_effort.to_string()),
        );
        Self {
            session_id,
            model_id: model_id.to_string(),
            meta: Some(meta),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
struct LegacySetModelResponse {}

/// Qwen extends the standard permission response with a top-level `answers`
/// object. Register the standard method through this lossless wrapper so
/// ordinary agents keep the exact ACP response while Qwen receives its
/// documented extension fields.
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(
    method = "session/request_permission",
    response = ExtendedRequestPermissionResponse
)]
#[serde(rename_all = "camelCase")]
struct ExtendedRequestPermissionRequest {
    session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_call: Option<ToolCallUpdate>,
    options: Vec<PermissionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    meta: Option<agent_client_protocol::schema::v1::Meta>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
struct ExtendedRequestPermissionResponse {
    #[serde(flatten)]
    standard: RequestPermissionResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    answers: Option<IndexMap<String, String>>,
}

impl ExtendedRequestPermissionResponse {
    fn new(outcome: RequestPermissionOutcome) -> Self {
        Self {
            standard: RequestPermissionResponse::new(outcome),
            answers: None,
        }
    }

    fn selected(option_id: impl Into<String>) -> Self {
        Self::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(option_id.into()),
        ))
    }

    fn cancelled() -> Self {
        Self::new(RequestPermissionOutcome::Cancelled)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct QwenQuestionOption {
    label: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct QwenQuestion {
    #[serde(default)]
    header: String,
    question: String,
    #[serde(default)]
    multi_select: bool,
    #[serde(default)]
    options: Vec<QwenQuestionOption>,
}

#[derive(Debug, Clone)]
struct QwenQuestionnaireContext {
    questions: Vec<QwenQuestion>,
    selected_option_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_x.ai/exit_plan_mode", response = GrokExitPlanModeResponse)]
#[serde(rename_all = "camelCase")]
struct GrokExitPlanModeRequest {
    session_id: SessionId,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    plan_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
struct GrokExitPlanModeResponse {
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    feedback: Option<String>,
}

impl GrokExitPlanModeResponse {
    fn new(outcome: impl Into<String>) -> Self {
        Self {
            outcome: outcome.into(),
            feedback: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_x.ai/ask_user_question", response = GrokAskUserResponse)]
#[serde(rename_all = "camelCase")]
struct GrokAskUserRequest {
    session_id: SessionId,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    questions: Vec<GrokQuestion>,
    #[serde(default)]
    mode: GrokAskUserMode,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum GrokAskUserMode {
    #[default]
    Default,
    Plan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrokQuestion {
    question: String,
    #[serde(default)]
    multi_select: bool,
    #[serde(default)]
    options: Vec<GrokQuestionOption>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrokQuestionOption {
    label: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    preview: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone)]
struct GrokQuestionnaireContext {
    questions: Vec<GrokQuestion>,
    mode: GrokAskUserMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrokQuestionAnnotation {
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum GrokAskUserResponse {
    Accepted {
        answers: IndexMap<String, Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        annotations: Option<IndexMap<String, GrokQuestionAnnotation>>,
    },
    ChatAboutThis {
        #[serde(default)]
        partial_answers: IndexMap<String, String>,
    },
    SkipInterview {
        #[serde(default)]
        partial_answers: IndexMap<String, String>,
    },
    Cancelled,
}

impl GrokAskUserResponse {
    fn cancelled() -> Self {
        Self::Cancelled
    }

    fn from_submission(
        request: &GrokAskUserRequest,
        submission: &AcpQuestionnaireSubmission,
    ) -> Self {
        if submission.outcome == AcpQuestionnaireOutcome::Cancelled {
            return Self::Cancelled;
        }

        if submission.outcome != AcpQuestionnaireOutcome::Accepted {
            let partial_answers = questionnaire_partial_answers(&request.questions, submission);
            return match submission.outcome {
                AcpQuestionnaireOutcome::ChatAboutThis => Self::ChatAboutThis { partial_answers },
                AcpQuestionnaireOutcome::SkipInterview => Self::SkipInterview { partial_answers },
                AcpQuestionnaireOutcome::Accepted
                | AcpQuestionnaireOutcome::Declined
                | AcpQuestionnaireOutcome::Cancelled => {
                    unreachable!("handled questionnaire outcome")
                }
            };
        }

        let mut answers = IndexMap::new();
        let mut annotations = IndexMap::new();
        let submitted = submission
            .answers
            .iter()
            .map(|answer| (answer.question_index, answer))
            .collect::<HashMap<_, _>>();
        for (question_index, question) in request.questions.iter().enumerate() {
            let Some(answer) = submitted.get(&question_index) else {
                continue;
            };
            let labels = selected_question_labels(question, answer);
            let notes = answer
                .other_text
                .as_ref()
                .filter(|text| !text.trim().is_empty())
                .cloned();
            if labels.is_empty() && notes.is_none() {
                continue;
            }
            answers.insert(
                question.question.clone(),
                if labels.is_empty() {
                    vec!["Other".into()]
                } else {
                    labels
                },
            );
            let preview = (!question.multi_select)
                .then(|| {
                    question
                        .options
                        .iter()
                        .enumerate()
                        .find(|(index, _)| answer.selected_option_indexes.contains(index))
                })
                .flatten()
                .and_then(|(_, option)| option.preview.clone());
            if preview.is_some() || notes.is_some() {
                annotations.insert(
                    question.question.clone(),
                    GrokQuestionAnnotation { preview, notes },
                );
            }
        }
        Self::Accepted {
            answers,
            annotations: (!annotations.is_empty()).then_some(annotations),
        }
    }
}

fn questionnaire_partial_answers(
    questions: &[GrokQuestion],
    submission: &AcpQuestionnaireSubmission,
) -> IndexMap<String, String> {
    let submitted = submission
        .answers
        .iter()
        .map(|answer| (answer.question_index, answer))
        .collect::<HashMap<_, _>>();
    questions
        .iter()
        .enumerate()
        .filter_map(|(question_index, question)| {
            let answer = submitted.get(&question_index)?;
            let labels = selected_question_labels(question, answer);
            // Grok's plan-only partial_answers wire type is a single string.
            // Preserve multi-select choices in their original option order via
            // an explicit AQBot compatibility convention instead of dropping
            // all but the first selection.
            let label = (!labels.is_empty())
                .then(|| labels.join(", "))
                .or_else(|| {
                    answer
                        .other_text
                        .as_deref()
                        .is_some_and(|text| !text.trim().is_empty())
                        .then(|| "Other".into())
                })?;
            Some((question.question.clone(), label))
        })
        .collect()
}

fn selected_question_labels(
    question: &GrokQuestion,
    answer: &AcpQuestionnaireAnswer,
) -> Vec<String> {
    question
        .options
        .iter()
        .enumerate()
        .filter(|(index, _)| answer.selected_option_indexes.contains(index))
        .map(|(_, option)| option.label.clone())
        .collect()
}

fn validate_questionnaire_submission(
    context: &GrokQuestionnaireContext,
    submission: &AcpQuestionnaireSubmission,
) -> Result<String, String> {
    if submission.outcome == AcpQuestionnaireOutcome::Declined {
        return Err("decline is only valid for a standard elicitation".into());
    }
    if matches!(
        submission.outcome,
        AcpQuestionnaireOutcome::ChatAboutThis | AcpQuestionnaireOutcome::SkipInterview
    ) && context.mode != GrokAskUserMode::Plan
    {
        return Err("plan-only questionnaire action used outside plan mode".into());
    }
    if submission.outcome == AcpQuestionnaireOutcome::Cancelled {
        return Ok(String::new());
    }

    let mut seen_questions = HashSet::new();
    let mut summary = Vec::new();
    for answer in &submission.answers {
        let Some(question) = context.questions.get(answer.question_index) else {
            return Err(format!(
                "question index {} is out of range",
                answer.question_index
            ));
        };
        if !seen_questions.insert(answer.question_index) {
            return Err(format!(
                "question index {} was answered more than once",
                answer.question_index
            ));
        }
        let mut seen_options = HashSet::new();
        for option_index in &answer.selected_option_indexes {
            if question.options.get(*option_index).is_none() {
                return Err(format!(
                    "option index {option_index} is out of range for question {}",
                    answer.question_index
                ));
            }
            if !seen_options.insert(*option_index) {
                return Err(format!(
                    "option index {option_index} was selected more than once"
                ));
            }
        }
        let other_text = answer
            .other_text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if !question.multi_select
            && (answer.selected_option_indexes.len() > 1
                || (!answer.selected_option_indexes.is_empty() && other_text.is_some()))
        {
            return Err(format!(
                "question {} only accepts one answer",
                answer.question_index
            ));
        }
        let mut labels = selected_question_labels(question, answer);
        if let Some(text) = other_text {
            labels.push(text.to_string());
        }
        if !labels.is_empty() {
            summary.push(format!("{}: {}", question.question, labels.join(", ")));
        }
    }
    Ok(summary.join("\n"))
}

fn qwen_response_from_submission(
    context: &QwenQuestionnaireContext,
    submission: &AcpQuestionnaireSubmission,
) -> Result<(String, ExtendedRequestPermissionResponse), String> {
    match submission.outcome {
        AcpQuestionnaireOutcome::Cancelled => {
            return Ok((
                String::new(),
                ExtendedRequestPermissionResponse::cancelled(),
            ));
        }
        AcpQuestionnaireOutcome::Accepted => {}
        AcpQuestionnaireOutcome::Declined
        | AcpQuestionnaireOutcome::ChatAboutThis
        | AcpQuestionnaireOutcome::SkipInterview => {
            return Err("unsupported outcome for a Qwen questionnaire".into());
        }
    }

    let mut submitted = HashMap::new();
    for answer in &submission.answers {
        if context.questions.get(answer.question_index).is_none() {
            return Err(format!(
                "question index {} is out of range",
                answer.question_index
            ));
        }
        if submitted.insert(answer.question_index, answer).is_some() {
            return Err(format!(
                "question index {} was answered more than once",
                answer.question_index
            ));
        }
    }

    let mut answers = IndexMap::new();
    let mut summary = Vec::new();
    for (question_index, question) in context.questions.iter().enumerate() {
        let answer = submitted
            .get(&question_index)
            .ok_or_else(|| format!("question {question_index} is required"))?;
        let mut seen = HashSet::new();
        let mut values = Vec::new();
        for option_index in &answer.selected_option_indexes {
            if !seen.insert(*option_index) {
                return Err(format!(
                    "option index {option_index} was selected more than once"
                ));
            }
            let option = question.options.get(*option_index).ok_or_else(|| {
                format!("option index {option_index} is out of range for question {question_index}")
            })?;
            values.push(option.label.clone());
        }
        let other = answer
            .other_text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if !question.multi_select && values.len() > 1 {
            return Err(format!("question {question_index} only accepts one answer"));
        }
        if let Some(other) = other {
            if question.multi_select {
                values.push(other.to_string());
            } else {
                values = vec![other.to_string()];
            }
        }
        if values.is_empty() {
            return Err(format!("question {question_index} is required"));
        }
        let display = values.join(", ");
        answers.insert(question_index.to_string(), display.clone());
        summary.push(format!("{}: {display}", question.question));
    }
    Ok((
        summary.join("\n"),
        ExtendedRequestPermissionResponse {
            standard: RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new(context.selected_option_id.clone()),
            )),
            answers: Some(answers),
        },
    ))
}

struct ValidatedElicitationValue {
    property_id: String,
    value: ElicitationContentValue,
    display: String,
}

fn selected_elicitation_options<'a>(
    question: &'a ElicitationQuestionContext,
    answer: &AcpQuestionnaireAnswer,
) -> Result<Vec<&'a ElicitationOptionContext>, String> {
    let mut seen = HashSet::new();
    answer
        .selected_option_indexes
        .iter()
        .map(|option_index| {
            if !seen.insert(*option_index) {
                return Err(format!(
                    "option index {option_index} was selected more than once"
                ));
            }
            question.options.get(*option_index).ok_or_else(|| {
                format!(
                    "option index {option_index} is out of range for question {}",
                    answer.question_index
                )
            })
        })
        .collect()
}

fn validate_string_elicitation(
    schema: &agent_client_protocol::schema::v1::StringPropertySchema,
    value: &str,
) -> Result<(), String> {
    let length = value.chars().count() as u32;
    if length as usize > MAX_ELICITATION_TEXT_CHARS {
        return Err("elicitation string exceeds the client safety limit".into());
    }
    if let Some(minimum) = schema.min_length {
        if length < minimum {
            return Err(format!("string is shorter than minimum length {minimum}"));
        }
    }
    if let Some(maximum) = schema.max_length {
        if length > maximum {
            return Err(format!("string is longer than maximum length {maximum}"));
        }
    }
    let allowed = schema
        .one_of
        .as_ref()
        .map(|options| options.iter().map(|option| option.value.as_str()).collect())
        .or_else(|| {
            schema
                .enum_values
                .as_ref()
                .map(|values| values.iter().map(String::as_str).collect())
        });
    if allowed.is_some_and(|allowed: Vec<&str>| !allowed.contains(&value)) {
        return Err("value is not one of the elicitation enum options".into());
    }
    if let Some(pattern) = schema.pattern.as_deref() {
        let pattern = regex::Regex::new(pattern)
            .map_err(|error| format!("invalid elicitation regex pattern: {error}"))?;
        if !pattern.is_match(value) {
            return Err("value does not match the elicitation pattern".into());
        }
    }
    if let Some(format) = schema.format.as_ref() {
        match format {
            StringFormat::Email => {
                let email = regex::Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$")
                    .expect("static email regex is valid");
                if !email.is_match(value) {
                    return Err("value is not a valid email address".into());
                }
            }
            StringFormat::Uri => {
                url::Url::parse(value)
                    .map_err(|_| "value is not a valid absolute URI".to_string())?;
            }
            StringFormat::Date => {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .map_err(|_| "value is not a valid ISO date".to_string())?;
            }
            StringFormat::DateTime => {
                chrono::DateTime::parse_from_rfc3339(value)
                    .map_err(|_| "value is not a valid RFC 3339 date-time".to_string())?;
            }
            _ => return Err("unsupported elicitation string format".into()),
        }
    }
    Ok(())
}

fn elicitation_scalar_value(
    schema: &ElicitationPropertySchema,
    value: &str,
) -> Result<ElicitationContentValue, String> {
    if value.chars().count() > MAX_ELICITATION_TEXT_CHARS {
        return Err("elicitation value exceeds the client safety limit".into());
    }
    match schema {
        ElicitationPropertySchema::String(schema) => {
            validate_string_elicitation(schema, value)?;
            Ok(value.to_string().into())
        }
        ElicitationPropertySchema::Number(schema) => {
            let parsed = value
                .parse::<f64>()
                .map_err(|_| "elicitation value is not a number".to_string())?;
            if !parsed.is_finite() {
                return Err("elicitation number must be finite".into());
            }
            if let Some(minimum) = schema.minimum {
                if parsed < minimum {
                    return Err(format!("number is below minimum {minimum}"));
                }
            }
            if let Some(maximum) = schema.maximum {
                if parsed > maximum {
                    return Err(format!("number is above maximum {maximum}"));
                }
            }
            Ok(parsed.into())
        }
        ElicitationPropertySchema::Integer(schema) => {
            let parsed = value
                .parse::<i64>()
                .map_err(|_| "elicitation value is not an integer".to_string())?;
            if let Some(minimum) = schema.minimum {
                if parsed < minimum {
                    return Err(format!("integer is below minimum {minimum}"));
                }
            }
            if let Some(maximum) = schema.maximum {
                if parsed > maximum {
                    return Err(format!("integer is above maximum {maximum}"));
                }
            }
            Ok(parsed.into())
        }
        ElicitationPropertySchema::Boolean(_) => match value {
            "true" => Ok(true.into()),
            "false" => Ok(false.into()),
            _ => Err("elicitation value is not a boolean".into()),
        },
        ElicitationPropertySchema::Array(_) => {
            Err("multi-select elicitation requires option selections".into())
        }
        _ => Err("unsupported elicitation property schema".into()),
    }
}

fn validate_elicitation_array(
    schema: &agent_client_protocol::schema::v1::MultiSelectPropertySchema,
    values: Vec<String>,
) -> Result<ElicitationContentValue, String> {
    let count = values.len() as u64;
    if let Some(minimum) = schema.min_items {
        if count < minimum {
            return Err(format!(
                "too few elicitation selections; minimum is {minimum}"
            ));
        }
    }
    if let Some(maximum) = schema.max_items {
        if count > maximum {
            return Err(format!(
                "too many elicitation selections; maximum is {maximum}"
            ));
        }
    }
    Ok(values.into())
}

fn elicitation_answer_value(
    question: &ElicitationQuestionContext,
    answer: &AcpQuestionnaireAnswer,
) -> Result<Option<ValidatedElicitationValue>, String> {
    let other_text = answer
        .other_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if let Some(text) = other_text {
        let (property_id, schema) = if let Some(other) = question.other.as_ref() {
            (&other.id, &other.schema)
        } else if question.options.is_empty() {
            (&question.id, &question.schema)
        } else {
            return Err(format!(
                "question {} does not allow a free-form answer",
                answer.question_index
            ));
        };
        let value = elicitation_scalar_value(schema, text)?;
        return Ok(Some(ValidatedElicitationValue {
            property_id: property_id.clone(),
            value,
            display: text.to_string(),
        }));
    }

    let selected = selected_elicitation_options(question, answer)?;
    if selected.is_empty() {
        return Ok(None);
    }

    if !matches!(question.schema, ElicitationPropertySchema::Array(_)) && selected.len() > 1 {
        return Err(format!(
            "question {} only accepts one answer",
            answer.question_index
        ));
    }
    let values = selected
        .iter()
        .map(|option| option.value.clone())
        .collect::<Vec<_>>();
    let display = selected
        .iter()
        .map(|option| option.label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let value = match &question.schema {
        ElicitationPropertySchema::Array(schema) => validate_elicitation_array(schema, values)?,
        _ => elicitation_scalar_value(&question.schema, &values[0])?,
    };
    Ok(Some(ValidatedElicitationValue {
        property_id: question.id.clone(),
        value,
        display,
    }))
}

fn accepted_elicitation_response(
    context: &ElicitationFormContext,
    submission: &AcpQuestionnaireSubmission,
) -> Result<(String, CreateElicitationResponse), String> {
    let mut submitted = HashMap::new();
    for answer in &submission.answers {
        if context.questions.get(answer.question_index).is_none() {
            return Err(format!(
                "question index {} is out of range",
                answer.question_index
            ));
        }
        if submitted.insert(answer.question_index, answer).is_some() {
            return Err(format!(
                "question index {} was answered more than once",
                answer.question_index
            ));
        }
    }

    let mut content = BTreeMap::new();
    let mut summary = Vec::new();
    for (question_index, question) in context.questions.iter().enumerate() {
        let value = submitted
            .get(&question_index)
            .map(|answer| elicitation_answer_value(question, answer))
            .transpose()?
            .flatten();
        let Some(value) = value else {
            if question.required {
                return Err(format!(
                    "required elicitation field `{}` is missing",
                    question.id
                ));
            }
            continue;
        };
        let display = if question.secret {
            "••••••".to_string()
        } else {
            value.display
        };
        summary.push(format!("{}: {display}", question.title));
        content.insert(value.property_id, value.value);
    }
    Ok((
        summary.join("\n"),
        CreateElicitationResponse::new(ElicitationAction::Accept(
            ElicitationAcceptAction::new().content(content),
        )),
    ))
}

fn elicitation_response_from_submission(
    context: &ElicitationFormContext,
    submission: &AcpQuestionnaireSubmission,
) -> Result<(String, CreateElicitationResponse), String> {
    match submission.outcome {
        AcpQuestionnaireOutcome::Accepted => accepted_elicitation_response(context, submission),
        AcpQuestionnaireOutcome::Declined => Ok((
            String::new(),
            CreateElicitationResponse::new(ElicitationAction::Decline),
        )),
        AcpQuestionnaireOutcome::Cancelled => Ok((
            String::new(),
            CreateElicitationResponse::new(ElicitationAction::Cancel),
        )),
        AcpQuestionnaireOutcome::ChatAboutThis | AcpQuestionnaireOutcome::SkipInterview => {
            Err("plan-only questionnaire action used for a standard elicitation".into())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "session/new", response = ExtendedNewSessionResponse)]
#[serde(rename_all = "camelCase")]
struct ExtendedNewSessionRequest {
    cwd: PathBuf,
    mcp_servers: Vec<McpServer>,
}

impl ExtendedNewSessionRequest {
    fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            mcp_servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
struct ExtendedNewSessionResponse {
    /// Keep the official response as the source of truth. Its deserializer
    /// deliberately skips malformed or future config-option variants instead
    /// of rejecting the whole `session/new` response.
    #[serde(flatten)]
    standard: NewSessionResponse,
    #[serde(default)]
    models: Option<serde_json::Value>,
    #[serde(default)]
    reasoning_efforts: Option<serde_json::Value>,
}
