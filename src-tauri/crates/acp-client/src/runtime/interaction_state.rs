#[derive(Debug, Clone)]
struct PermissionResolution {
    option_id: String,
    feedback: Option<String>,
}

struct PendingPermission {
    scope: String,
    interaction_kind: AcpInteractionKind,
    tool_call_id: Option<String>,
    options: Vec<PermissionOptionView>,
    questionnaire: Option<PendingQuestionnaire>,
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    sender: Option<oneshot::Sender<PermissionResolution>>,
}

enum PendingQuestionnaire {
    Grok {
        context: GrokQuestionnaireContext,
        sender: Option<oneshot::Sender<AcpQuestionnaireSubmission>>,
    },
    Elicitation {
        context: ElicitationFormContext,
        sender: Option<oneshot::Sender<CreateElicitationResponse>>,
    },
    Qwen {
        context: QwenQuestionnaireContext,
        sender: Option<oneshot::Sender<ExtendedRequestPermissionResponse>>,
    },
}

#[derive(Debug, Clone)]
struct ElicitationFormContext {
    questions: Vec<ElicitationQuestionContext>,
}

#[derive(Debug, Clone)]
struct ElicitationQuestionContext {
    id: String,
    title: String,
    required: bool,
    secret: bool,
    schema: ElicitationPropertySchema,
    options: Vec<ElicitationOptionContext>,
    other: Option<ElicitationOtherPropertyContext>,
}

#[derive(Debug, Clone)]
struct ElicitationOtherPropertyContext {
    id: String,
    schema: ElicitationPropertySchema,
}

#[derive(Debug, Clone)]
struct ElicitationOptionContext {
    value: String,
    label: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcpQuestionnaireOutcome {
    Accepted,
    Declined,
    ChatAboutThis,
    SkipInterview,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpQuestionnaireAnswer {
    pub question_index: usize,
    #[serde(default)]
    pub selected_option_indexes: Vec<usize>,
    #[serde(default)]
    pub other_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpQuestionnaireSubmission {
    pub outcome: AcpQuestionnaireOutcome,
    #[serde(default)]
    pub answers: Vec<AcpQuestionnaireAnswer>,
}

type PermissionMap = Arc<Mutex<HashMap<String, PendingPermission>>>;
type EventTxSlot = Arc<Mutex<Option<mpsc::UnboundedSender<AcpEvent>>>>;
type ConnectionSlot = Arc<Mutex<Option<ConnectionTo<Agent>>>>;
type RouteMap = Arc<Mutex<SessionRoutes>>;

fn emit_interaction_closed(
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
    request_id: &str,
    interaction_kind: AcpInteractionKind,
    tool_call_id: Option<String>,
    outcome: AcpInteractionOutcome,
    selected: Option<&PermissionOptionView>,
) {
    if let Err(error) = event_tx.send(AcpEvent::InteractionClosed {
        request_id: request_id.to_string(),
        interaction_kind,
        tool_call_id,
        outcome,
        selected_option_id: selected.map(|option| option.option_id.clone()),
        selected_option_kind: selected.and_then(|option| option.kind.clone()),
        selected_option_name: selected.map(|option| option.name.clone()),
    }) {
        tracing::warn!(%error, request_id, "failed to emit ACP interaction terminal event");
    }
}

async fn expire_permission(permissions: &PermissionMap, request_id: &str) {
    let pending = permissions.lock().await.remove(request_id);
    if let Some(pending) = pending {
        emit_interaction_closed(
            &pending.event_tx,
            request_id,
            pending.interaction_kind,
            pending.tool_call_id.clone(),
            AcpInteractionOutcome::Expired,
            None,
        );
    }
}

async fn cancel_permission_scope(permissions: &PermissionMap, scope: &str) {
    let mut permissions = permissions.lock().await;
    let request_ids = permissions
        .iter()
        .filter(|(_, pending)| pending.scope == scope)
        .map(|(request_id, _)| request_id.clone())
        .collect::<Vec<_>>();
    let cancelled = request_ids
        .into_iter()
        .filter_map(|request_id| {
            permissions
                .remove(&request_id)
                .map(|pending| (request_id, pending))
        })
        .collect::<Vec<_>>();
    drop(permissions);
    for (request_id, pending) in cancelled {
        emit_interaction_closed(
            &pending.event_tx,
            &request_id,
            pending.interaction_kind,
            pending.tool_call_id.clone(),
            AcpInteractionOutcome::Cancelled,
            None,
        );
    }
}
