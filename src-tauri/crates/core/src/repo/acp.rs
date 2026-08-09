use crate::entity::{acp_messages, acp_projects, acp_threads};
use crate::error::{AQBotError, Result};
use crate::file_store::FileStore;
use crate::types::{Attachment, AttachmentInput};
use crate::utils::gen_id;
use sea_orm::sea_query::Expr;
use sea_orm::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

fn now_str() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpMessageView {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub status: Option<String>,
    pub attachments: Vec<Attachment>,
    pub meta_json: Option<String>,
    pub created_at: String,
}

pub struct AcpPromptFinalization<'a> {
    pub thread_id: &'a str,
    pub message_id: &'a str,
    pub content: &'a str,
    pub message_status: &'a str,
    pub meta_json: Option<&'a str>,
    pub acp_session_id: Option<&'a str>,
    pub runtime_status: &'a str,
}

fn parse_attachments(message_id: &str, value: Option<&str>) -> Result<Vec<Attachment>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let attachments: Vec<Attachment> = serde_json::from_str(value).map_err(|error| {
        AQBotError::Validation(format!(
            "Invalid ACP message {message_id} attachments JSON: {error}"
        ))
    })?;
    for attachment in &attachments {
        if attachment.data.is_some() {
            return Err(AQBotError::Validation(format!(
                "ACP message {message_id} attachment metadata contains inline data"
            )));
        }
        if attachment.id.is_empty() {
            return Err(AQBotError::Validation(format!(
                "ACP message {message_id} attachment has no stored file id"
            )));
        }
        FileStore::new()
            .validated_path(&attachment.file_path)
            .map_err(|error| {
                AQBotError::Validation(format!(
                    "ACP message {message_id} attachment path is invalid: {error}"
                ))
            })?;
    }
    Ok(attachments)
}

fn message_view(model: acp_messages::Model) -> Result<AcpMessageView> {
    let attachments = parse_attachments(&model.id, model.attachments_json.as_deref())?;
    Ok(AcpMessageView {
        id: model.id,
        thread_id: model.thread_id,
        role: model.role,
        content: model.content,
        status: model.status,
        attachments,
        meta_json: model.meta_json,
        created_at: model.created_at,
    })
}

fn collect_message_file_ids(rows: &[acp_messages::Model]) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for row in rows {
        ids.extend(crate::repo::stored_file::stored_media_ids(&row.content));
        ids.extend(
            parse_attachments(&row.id, row.attachments_json.as_deref())?
                .into_iter()
                .map(|attachment| attachment.id),
        );
    }
    Ok(ids)
}

async fn thread_has_streaming_prompt<C>(db: &C, thread_id: &str) -> Result<bool>
where
    C: ConnectionTrait,
{
    Ok(acp_messages::Entity::find()
        .filter(acp_messages::Column::ThreadId.eq(thread_id))
        .filter(acp_messages::Column::Role.eq("assistant"))
        .filter(acp_messages::Column::Status.eq("streaming"))
        .one(db)
        .await?
        .is_some())
}

fn transaction_failure(primary: AQBotError, rollback: Option<DbErr>) -> AQBotError {
    match rollback {
        None => primary,
        Some(rollback) => AQBotError::Validation(format!(
            "{primary}; transaction rollback failed: {rollback}"
        )),
    }
}

fn cleanup_paths(file_store: &FileStore, paths: Vec<String>) -> Result<()> {
    let failures = paths
        .into_iter()
        .filter_map(|path| {
            file_store
                .delete_file(&path)
                .err()
                .map(|error| format!("{path}: {error}"))
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AQBotError::Validation(format!(
            "ACP database changes committed, but backing file cleanup failed: {}",
            failures.join(", ")
        )))
    }
}

// --- Projects ---

pub async fn list_projects(db: &DatabaseConnection) -> Result<Vec<acp_projects::Model>> {
    // Same idea as conversation categories: stable user order via sort_order
    Ok(acp_projects::Entity::find()
        .order_by_asc(acp_projects::Column::SortOrder)
        .order_by_asc(acp_projects::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn create_project(
    db: &DatabaseConnection,
    name: &str,
    root_path: &str,
) -> Result<acp_projects::Model> {
    create_project_with_kind(db, name, root_path, "project").await
}

pub async fn create_recent_workspace(
    db: &DatabaseConnection,
    name: &str,
    root_path: &str,
) -> Result<acp_projects::Model> {
    create_project_with_kind(db, name, root_path, "recent").await
}

pub async fn create_recent_draft_workspace(
    db: &DatabaseConnection,
    name: &str,
    root_path: &str,
) -> Result<acp_projects::Model> {
    create_project_with_kind(db, name, root_path, "recent_draft").await
}

async fn create_project_with_kind(
    db: &DatabaseConnection,
    name: &str,
    root_path: &str,
    kind: &str,
) -> Result<acp_projects::Model> {
    let now = now_str();
    let max_order = acp_projects::Entity::find()
        .order_by_desc(acp_projects::Column::SortOrder)
        .one(db)
        .await?
        .map(|p| p.sort_order)
        .unwrap_or(-1);
    let model = acp_projects::ActiveModel {
        id: Set(gen_id()),
        name: Set(name.to_string()),
        root_path: Set(root_path.to_string()),
        kind: Set(kind.to_string()),
        sort_order: Set(max_order + 1),
        created_at: Set(now.clone()),
        updated_at: Set(now.clone()),
        last_opened_at: Set(Some(now)),
    };
    Ok(model.insert(db).await?)
}

/// Persist project order — mirrors `reorder_conversation_categories`.
pub async fn reorder_projects(db: &DatabaseConnection, project_ids: &[String]) -> Result<()> {
    for (i, id) in project_ids.iter().enumerate() {
        if let Some(model) = get_project(db, id).await? {
            let mut am: acp_projects::ActiveModel = model.into();
            am.sort_order = Set(i as i32);
            am.updated_at = Set(now_str());
            am.update(db).await?;
        }
    }
    Ok(())
}

pub async fn get_project(db: &DatabaseConnection, id: &str) -> Result<Option<acp_projects::Model>> {
    Ok(acp_projects::Entity::find_by_id(id.to_string())
        .one(db)
        .await?)
}

pub async fn touch_project(db: &DatabaseConnection, id: &str) -> Result<()> {
    let now = now_str();
    if let Some(model) = get_project(db, id).await? {
        let mut am: acp_projects::ActiveModel = model.into();
        am.last_opened_at = Set(Some(now.clone()));
        am.updated_at = Set(now);
        am.update(db).await?;
    }
    Ok(())
}

/// Update project name and/or root path (settings modal).
pub async fn update_project(
    db: &DatabaseConnection,
    id: &str,
    name: Option<&str>,
    root_path: Option<&str>,
) -> Result<Option<acp_projects::Model>> {
    let Some(model) = get_project(db, id).await? else {
        return Ok(None);
    };
    let mut am: acp_projects::ActiveModel = model.into();
    if let Some(n) = name {
        let trimmed = n.trim();
        if !trimmed.is_empty() {
            am.name = Set(trimmed.to_string());
        }
    }
    if let Some(path) = root_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            am.root_path = Set(trimmed.to_string());
        }
    }
    am.updated_at = Set(now_str());
    Ok(Some(am.update(db).await?))
}

pub async fn delete_project(db: &DatabaseConnection, id: &str) -> Result<()> {
    delete_project_using(db, &FileStore::new(), id).await
}

async fn delete_project_using(
    db: &DatabaseConnection,
    file_store: &FileStore,
    id: &str,
) -> Result<()> {
    let _file_reference_guard = crate::repo::stored_file::lock_file_references().await;
    let txn = db.begin().await?;
    let operation = async {
        if acp_projects::Entity::find_by_id(id)
            .one(&txn)
            .await?
            .is_none()
        {
            return Err(AQBotError::NotFound(format!("ACP project {id}")));
        }
        let threads = acp_threads::Entity::find()
            .filter(acp_threads::Column::ProjectId.eq(id))
            .all(&txn)
            .await?;
        let thread_ids = threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>();
        let has_streaming_prompt = if thread_ids.is_empty() {
            false
        } else {
            acp_messages::Entity::find()
                .filter(acp_messages::Column::ThreadId.is_in(thread_ids.clone()))
                .filter(acp_messages::Column::Role.eq("assistant"))
                .filter(acp_messages::Column::Status.eq("streaming"))
                .one(&txn)
                .await?
                .is_some()
        };
        if has_streaming_prompt
            || threads
                .iter()
                .any(|thread| thread.runtime_status == "running")
        {
            return Err(AQBotError::Validation(
                "Cannot delete an ACP project while one of its prompts is running".to_string(),
            ));
        }
        let rows = if thread_ids.is_empty() {
            Vec::new()
        } else {
            acp_messages::Entity::find()
                .filter(acp_messages::Column::ThreadId.is_in(thread_ids.clone()))
                .all(&txn)
                .await?
        };
        let candidates = collect_message_file_ids(&rows)?;
        if !thread_ids.is_empty() {
            acp_messages::Entity::delete_many()
                .filter(acp_messages::Column::ThreadId.is_in(thread_ids.clone()))
                .exec(&txn)
                .await?;
            acp_threads::Entity::delete_many()
                .filter(acp_threads::Column::Id.is_in(thread_ids))
                .exec(&txn)
                .await?;
        }
        acp_projects::Entity::delete_by_id(id).exec(&txn).await?;
        crate::repo::stored_file::delete_unreferenced_candidates(&txn, &candidates).await
    }
    .await;
    let paths = match operation {
        Ok(paths) => paths,
        Err(error) => {
            let rollback = txn.rollback().await.err();
            return Err(transaction_failure(error, rollback));
        }
    };
    txn.commit().await?;
    cleanup_paths(file_store, paths)
}

// --- Threads ---

pub async fn list_threads_for_project(
    db: &DatabaseConnection,
    project_id: &str,
) -> Result<Vec<acp_threads::Model>> {
    Ok(acp_threads::Entity::find()
        .filter(acp_threads::Column::ProjectId.eq(project_id))
        .order_by_desc(acp_threads::Column::IsPinned)
        .order_by_asc(acp_threads::Column::SortOrder)
        .order_by_desc(acp_threads::Column::UpdatedAt)
        .all(db)
        .await?)
}

pub async fn list_all_threads(db: &DatabaseConnection) -> Result<Vec<acp_threads::Model>> {
    // Per-project pin/sort; clients filter by project_id for grouping.
    Ok(acp_threads::Entity::find()
        .order_by_desc(acp_threads::Column::IsPinned)
        .order_by_asc(acp_threads::Column::SortOrder)
        .order_by_desc(acp_threads::Column::UpdatedAt)
        .all(db)
        .await?)
}

pub async fn create_thread(
    db: &DatabaseConnection,
    project_id: &str,
    agent_id: &str,
    title: &str,
) -> Result<acp_threads::Model> {
    // New threads appear at the top of the unpinned group
    let min_order = acp_threads::Entity::find()
        .filter(acp_threads::Column::ProjectId.eq(project_id))
        .filter(acp_threads::Column::IsPinned.eq(0))
        .order_by_asc(acp_threads::Column::SortOrder)
        .one(db)
        .await?
        .map(|t| t.sort_order)
        .unwrap_or(0);
    Ok(new_thread_model(project_id, agent_id, title, min_order - 1)
        .insert(db)
        .await?)
}

fn new_thread_model(
    project_id: &str,
    agent_id: &str,
    title: &str,
    sort_order: i32,
) -> acp_threads::ActiveModel {
    let now = now_str();
    acp_threads::ActiveModel {
        id: Set(gen_id()),
        project_id: Set(project_id.to_string()),
        agent_id: Set(agent_id.to_string()),
        title: Set(title.to_string()),
        acp_session_id: Set(None),
        runtime_status: Set("idle".into()),
        mode_id: Set(None),
        is_pinned: Set(0),
        sort_order: Set(sort_order),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    }
}

/// Atomically turn one hidden Recent draft workspace into a visible thread.
/// The kind transition distinguishes intentional drafts from empty Recent
/// projects left behind by an interrupted deletion.
pub async fn claim_recent_draft_thread(
    db: &DatabaseConnection,
    project_id: &str,
    agent_id: &str,
    title: &str,
    session_id: Option<&str>,
    mode_id: Option<&str>,
) -> Result<acp_threads::Model> {
    let txn = db.begin().await?;
    let operation = async {
        let project = acp_projects::Entity::find_by_id(project_id)
            .one(&txn)
            .await?
            .ok_or_else(|| AQBotError::NotFound(format!("ACP project {project_id}")))?;
        if project.kind != "recent_draft" {
            return Err(AQBotError::Validation(format!(
                "ACP project {project_id} is no longer an unclaimed Recent draft"
            )));
        }
        if acp_threads::Entity::find()
            .filter(acp_threads::Column::ProjectId.eq(project_id))
            .one(&txn)
            .await?
            .is_some()
        {
            return Err(AQBotError::Validation(format!(
                "ACP Recent draft {project_id} already owns a thread"
            )));
        }

        let mut project_update: acp_projects::ActiveModel = project.into();
        project_update.kind = Set("recent".to_string());
        project_update.name = Set(title.to_string());
        project_update.updated_at = Set(now_str());
        project_update.update(&txn).await?;
        let mut thread = new_thread_model(project_id, agent_id, title, -1);
        thread.acp_session_id = Set(session_id.map(str::to_string));
        thread.mode_id = Set(mode_id.map(str::to_string));
        Ok(thread.insert(&txn).await?)
    }
    .await;
    let thread = match operation {
        Ok(thread) => thread,
        Err(error) => {
            let rollback = txn.rollback().await.err();
            return Err(transaction_failure(error, rollback));
        }
    };
    txn.commit().await?;
    Ok(thread)
}

pub async fn get_thread(db: &DatabaseConnection, id: &str) -> Result<Option<acp_threads::Model>> {
    Ok(acp_threads::Entity::find_by_id(id.to_string())
        .one(db)
        .await?)
}

pub async fn update_thread_session(
    db: &DatabaseConnection,
    id: &str,
    acp_session_id: Option<&str>,
    runtime_status: &str,
) -> Result<()> {
    if let Some(model) = get_thread(db, id).await? {
        let mut am: acp_threads::ActiveModel = model.into();
        if let Some(sid) = acp_session_id {
            am.acp_session_id = Set(Some(sid.to_string()));
        }
        am.runtime_status = Set(runtime_status.to_string());
        am.updated_at = Set(now_str());
        am.update(db).await?;
    }
    Ok(())
}

pub async fn update_thread_session_id(
    db: &DatabaseConnection,
    id: &str,
    acp_session_id: &str,
) -> Result<()> {
    if let Some(model) = get_thread(db, id).await? {
        let mut am: acp_threads::ActiveModel = model.into();
        am.acp_session_id = Set(Some(acp_session_id.to_string()));
        am.updated_at = Set(now_str());
        am.update(db).await?;
    }
    Ok(())
}

/// Atomically persist the session identity and mode only while the thread
/// still exists. The affected-row result closes the prepare/delete race that
/// a read-then-update sequence cannot detect.
pub async fn persist_prepared_thread_session(
    db: &DatabaseConnection,
    id: &str,
    acp_session_id: &str,
    mode_id: Option<&str>,
) -> Result<bool> {
    let result = acp_threads::Entity::update_many()
        .col_expr(
            acp_threads::Column::AcpSessionId,
            Expr::value(Some(acp_session_id.to_string())),
        )
        .col_expr(
            acp_threads::Column::ModeId,
            Expr::value(mode_id.map(str::to_string)),
        )
        .col_expr(acp_threads::Column::UpdatedAt, Expr::value(now_str()))
        .filter(acp_threads::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

pub async fn update_thread_mode(
    db: &DatabaseConnection,
    id: &str,
    mode_id: Option<&str>,
) -> Result<()> {
    if let Some(model) = get_thread(db, id).await? {
        let mut am: acp_threads::ActiveModel = model.into();
        am.mode_id = Set(mode_id.map(str::to_string));
        am.updated_at = Set(now_str());
        am.update(db).await?;
    }
    Ok(())
}

pub async fn update_thread_title(
    db: &DatabaseConnection,
    id: &str,
    title: &str,
) -> Result<Option<acp_threads::Model>> {
    let Some(model) = get_thread(db, id).await? else {
        return Ok(None);
    };
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Ok(Some(model));
    }
    let mut am: acp_threads::ActiveModel = model.into();
    am.title = Set(trimmed.to_string());
    am.updated_at = Set(now_str());
    Ok(Some(am.update(db).await?))
}

pub async fn toggle_thread_pin(
    db: &DatabaseConnection,
    id: &str,
) -> Result<Option<acp_threads::Model>> {
    let Some(model) = get_thread(db, id).await? else {
        return Ok(None);
    };
    let next = if model.is_pinned != 0 { 0 } else { 1 };
    let mut am: acp_threads::ActiveModel = model.into();
    am.is_pinned = Set(next);
    am.updated_at = Set(now_str());
    Ok(Some(am.update(db).await?))
}

/// Persist thread order within a project (after pin grouping is applied client-side).
pub async fn reorder_threads(
    db: &DatabaseConnection,
    project_id: &str,
    thread_ids: &[String],
) -> Result<()> {
    let now = now_str();
    for (i, id) in thread_ids.iter().enumerate() {
        if let Some(model) = get_thread(db, id).await? {
            if model.project_id != project_id {
                continue;
            }
            let mut am: acp_threads::ActiveModel = model.into();
            am.sort_order = Set(i as i32);
            am.updated_at = Set(now.clone());
            am.update(db).await?;
        }
    }
    Ok(())
}

/// Duplicate a thread and all of its messages into a new idle thread (no live session).
pub async fn duplicate_thread(
    db: &DatabaseConnection,
    id: &str,
    title_suffix: &str,
) -> Result<Option<acp_threads::Model>> {
    let _file_reference_guard = crate::repo::stored_file::lock_file_references().await;
    let txn = db.begin().await?;
    let operation = async {
        let Some(source) = acp_threads::Entity::find_by_id(id).one(&txn).await? else {
            return Ok(None);
        };
        let messages = list_message_models(&txn, id).await?;
        // Refuse to duplicate corrupt attachment metadata. The copied thread
        // shares stored-file IDs, so the reference lock must span the commit.
        collect_message_file_ids(&messages)?;
        let now = now_str();
        let copy_title = if title_suffix.is_empty() {
            source.title.clone()
        } else {
            format!("{}{}", source.title, title_suffix)
        };
        let min_order = acp_threads::Entity::find()
            .filter(acp_threads::Column::ProjectId.eq(&source.project_id))
            .filter(acp_threads::Column::IsPinned.eq(0))
            .order_by_asc(acp_threads::Column::SortOrder)
            .one(&txn)
            .await?
            .map(|thread| thread.sort_order)
            .unwrap_or(0);
        let inserted = acp_threads::ActiveModel {
            id: Set(gen_id()),
            project_id: Set(source.project_id.clone()),
            agent_id: Set(source.agent_id.clone()),
            title: Set(copy_title),
            acp_session_id: Set(None),
            runtime_status: Set("idle".into()),
            mode_id: Set(source.mode_id.clone()),
            is_pinned: Set(0),
            sort_order: Set(min_order - 1),
            created_at: Set(now.clone()),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;
        for message in messages {
            acp_messages::ActiveModel {
                id: Set(gen_id()),
                thread_id: Set(inserted.id.clone()),
                role: Set(message.role),
                content: Set(message.content),
                status: Set(message.status),
                attachments_json: Set(message.attachments_json),
                meta_json: Set(message.meta_json),
                created_at: Set(message.created_at),
            }
            .insert(&txn)
            .await?;
        }
        Ok(Some(inserted))
    }
    .await;
    let inserted = match operation {
        Ok(inserted) => inserted,
        Err(error) => {
            let rollback = txn.rollback().await.err();
            return Err(transaction_failure(error, rollback));
        }
    };
    txn.commit().await?;
    Ok(inserted)
}

pub async fn delete_thread(db: &DatabaseConnection, id: &str) -> Result<()> {
    delete_thread_using(db, &FileStore::new(), id).await
}

async fn delete_thread_using(
    db: &DatabaseConnection,
    file_store: &FileStore,
    id: &str,
) -> Result<()> {
    let _file_reference_guard = crate::repo::stored_file::lock_file_references().await;
    let txn = db.begin().await?;
    let operation = async {
        let Some(thread) = acp_threads::Entity::find_by_id(id).one(&txn).await? else {
            return Err(AQBotError::NotFound(format!("ACP thread {id}")));
        };
        if thread.runtime_status == "running" || thread_has_streaming_prompt(&txn, id).await? {
            return Err(AQBotError::Validation(
                "Cannot delete an ACP thread while its prompt is running".to_string(),
            ));
        }
        let rows = list_message_models(&txn, id).await?;
        let candidates = collect_message_file_ids(&rows)?;
        acp_messages::Entity::delete_many()
            .filter(acp_messages::Column::ThreadId.eq(id))
            .exec(&txn)
            .await?;
        acp_threads::Entity::delete_by_id(id).exec(&txn).await?;
        crate::repo::stored_file::delete_unreferenced_candidates(&txn, &candidates).await
    }
    .await;
    let paths = match operation {
        Ok(paths) => paths,
        Err(error) => {
            let rollback = txn.rollback().await.err();
            return Err(transaction_failure(error, rollback));
        }
    };
    txn.commit().await?;
    cleanup_paths(file_store, paths)
}

// --- Messages ---

async fn list_message_models<C>(db: &C, thread_id: &str) -> Result<Vec<acp_messages::Model>>
where
    C: ConnectionTrait,
{
    Ok(acp_messages::Entity::find()
        .filter(acp_messages::Column::ThreadId.eq(thread_id))
        .order_by_asc(acp_messages::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn list_messages(
    db: &DatabaseConnection,
    thread_id: &str,
) -> Result<Vec<AcpMessageView>> {
    list_message_models(db, thread_id)
        .await?
        .into_iter()
        .map(message_view)
        .collect()
}

pub async fn interrupt_streaming_messages(
    db: &DatabaseConnection,
    thread_id: &str,
    reason: &str,
) -> Result<u64> {
    let rows = acp_messages::Entity::find()
        .filter(acp_messages::Column::ThreadId.eq(thread_id))
        .filter(acp_messages::Column::Role.eq("assistant"))
        .filter(acp_messages::Column::Status.eq("streaming"))
        .all(db)
        .await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let txn = db.begin().await?;
    for row in &rows {
        let content = if row.content.trim().is_empty() {
            format!("Error: {reason}")
        } else {
            format!("{}\n\nError: {reason}", row.content)
        };
        let mut update: acp_messages::ActiveModel = row.clone().into();
        update.content = Set(content);
        update.status = Set(Some("error".to_string()));
        update.update(&txn).await?;
    }
    if let Some(thread) = acp_threads::Entity::find_by_id(thread_id).one(&txn).await? {
        let mut update: acp_threads::ActiveModel = thread.into();
        update.runtime_status = Set("error".to_string());
        update.updated_at = Set(now_str());
        update.update(&txn).await?;
    }
    txn.commit().await?;
    Ok(rows.len() as u64)
}

pub async fn interrupt_all_streaming_messages(
    db: &DatabaseConnection,
    reason: &str,
) -> Result<u64> {
    let thread_ids = acp_messages::Entity::find()
        .filter(acp_messages::Column::Role.eq("assistant"))
        .filter(acp_messages::Column::Status.eq("streaming"))
        .all(db)
        .await?
        .into_iter()
        .map(|message| message.thread_id)
        .collect::<HashSet<_>>();
    let mut interrupted = 0;
    for thread_id in thread_ids {
        interrupted += interrupt_streaming_messages(db, &thread_id, reason).await?;
    }
    Ok(interrupted)
}

async fn insert_message<C>(
    db: &C,
    thread_id: &str,
    role: &str,
    content: &str,
    status: Option<&str>,
    attachments_json: Option<&str>,
    meta_json: Option<&str>,
) -> Result<acp_messages::Model>
where
    C: ConnectionTrait,
{
    let model = acp_messages::ActiveModel {
        id: Set(gen_id()),
        thread_id: Set(thread_id.to_string()),
        role: Set(role.to_string()),
        content: Set(content.to_string()),
        status: Set(status.map(|s| s.to_string())),
        attachments_json: Set(attachments_json.map(str::to_string)),
        meta_json: Set(meta_json.map(|s| s.to_string())),
        created_at: Set(now_str()),
    };
    Ok(model.insert(db).await?)
}

pub async fn create_prompt_messages(
    db: &DatabaseConnection,
    thread_id: &str,
    content: &str,
    inputs: &[AttachmentInput],
) -> Result<(AcpMessageView, AcpMessageView)> {
    crate::storage_paths::ensure_documents_dirs()?;
    create_prompt_messages_using(db, &FileStore::new(), thread_id, content, inputs).await
}

async fn create_prompt_messages_using(
    db: &DatabaseConnection,
    file_store: &FileStore,
    thread_id: &str,
    content: &str,
    inputs: &[AttachmentInput],
) -> Result<(AcpMessageView, AcpMessageView)> {
    if content.trim().is_empty() && inputs.is_empty() {
        return Err(AQBotError::Validation(
            "ACP prompt must contain text or attachments".to_string(),
        ));
    }
    let _file_reference_guard = crate::repo::stored_file::lock_file_references().await;
    let txn = db.begin().await?;
    let mut created_paths = Vec::new();
    let operation = async {
        let Some(thread) = acp_threads::Entity::find_by_id(thread_id).one(&txn).await? else {
            return Err(AQBotError::NotFound(format!("ACP thread {thread_id}")));
        };
        if thread.runtime_status == "running"
            || thread_has_streaming_prompt(&txn, thread_id).await?
        {
            return Err(AQBotError::Validation(
                "Cannot send another ACP prompt while one is running".to_string(),
            ));
        }
        let attachments = crate::attachment_persistence::persist_attachments_in_transaction(
            &txn,
            file_store,
            None,
            inputs,
            &mut created_paths,
        )
        .await?;
        let attachments_json = if attachments.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&attachments).map_err(|error| {
                AQBotError::Validation(format!(
                    "Failed to serialize ACP prompt attachments: {error}"
                ))
            })?)
        };
        let user = insert_message(
            &txn,
            thread_id,
            "user",
            content,
            Some("done"),
            attachments_json.as_deref(),
            None,
        )
        .await?;
        let assistant = insert_message(
            &txn,
            thread_id,
            "assistant",
            "",
            Some("streaming"),
            None,
            None,
        )
        .await?;
        let mut thread_update: acp_threads::ActiveModel = thread.into();
        thread_update.runtime_status = Set("running".to_string());
        thread_update.updated_at = Set(now_str());
        thread_update.update(&txn).await?;
        Ok((message_view(user)?, message_view(assistant)?))
    }
    .await;
    let messages = match operation {
        Ok(messages) => messages,
        Err(error) => {
            let rollback = txn.rollback().await.err();
            let cleanup = crate::attachment_persistence::cleanup_created_paths(
                db,
                file_store,
                &created_paths,
            )
            .await;
            let primary = transaction_failure(error, rollback);
            if cleanup.is_empty() {
                return Err(primary);
            }
            return Err(AQBotError::Validation(format!(
                "{primary}; physical rollback failed: {}",
                cleanup.join(", ")
            )));
        }
    };
    if let Err(error) = txn.commit().await {
        let cleanup =
            crate::attachment_persistence::cleanup_created_paths(db, file_store, &created_paths)
                .await;
        let primary = AQBotError::from(error);
        if cleanup.is_empty() {
            return Err(primary);
        }
        return Err(AQBotError::Validation(format!(
            "{primary}; physical rollback failed: {}",
            cleanup.join(", ")
        )));
    }
    Ok(messages)
}

pub async fn rollback_prompt_messages(
    db: &DatabaseConnection,
    thread_id: &str,
    message_ids: &[String],
) -> Result<()> {
    rollback_prompt_messages_using(db, &FileStore::new(), thread_id, message_ids).await
}

async fn rollback_prompt_messages_using(
    db: &DatabaseConnection,
    file_store: &FileStore,
    thread_id: &str,
    message_ids: &[String],
) -> Result<()> {
    let _file_reference_guard = crate::repo::stored_file::lock_file_references().await;
    let txn = db.begin().await?;
    let operation = async {
        let rows = if message_ids.is_empty() {
            Vec::new()
        } else {
            acp_messages::Entity::find()
                .filter(acp_messages::Column::ThreadId.eq(thread_id))
                .filter(acp_messages::Column::Id.is_in(message_ids.to_vec()))
                .all(&txn)
                .await?
        };
        let candidates = collect_message_file_ids(&rows)?;
        if !message_ids.is_empty() {
            acp_messages::Entity::delete_many()
                .filter(acp_messages::Column::ThreadId.eq(thread_id))
                .filter(acp_messages::Column::Id.is_in(message_ids.to_vec()))
                .exec(&txn)
                .await?;
        }
        let another_prompt_is_running = thread_has_streaming_prompt(&txn, thread_id).await?;
        if let Some(thread) = acp_threads::Entity::find_by_id(thread_id).one(&txn).await? {
            let mut update: acp_threads::ActiveModel = thread.into();
            update.runtime_status = Set(if another_prompt_is_running {
                "running".to_string()
            } else {
                "idle".to_string()
            });
            update.updated_at = Set(now_str());
            update.update(&txn).await?;
        }
        crate::repo::stored_file::delete_unreferenced_candidates(&txn, &candidates).await
    }
    .await;
    let paths = match operation {
        Ok(paths) => paths,
        Err(error) => {
            let rollback = txn.rollback().await.err();
            return Err(transaction_failure(error, rollback));
        }
    };
    txn.commit().await?;
    cleanup_paths(file_store, paths)
}

pub async fn update_message_content(
    db: &DatabaseConnection,
    id: &str,
    content: &str,
    status: Option<&str>,
    meta_json: Option<&str>,
) -> Result<()> {
    if let Some(model) = acp_messages::Entity::find_by_id(id.to_string())
        .one(db)
        .await?
    {
        let mut am: acp_messages::ActiveModel = model.into();
        am.content = Set(content.to_string());
        if let Some(s) = status {
            am.status = Set(Some(s.to_string()));
        }
        if let Some(m) = meta_json {
            am.meta_json = Set(Some(m.to_string()));
        }
        am.update(db).await?;
    }
    Ok(())
}

pub async fn finalize_prompt(
    db: &DatabaseConnection,
    finalization: AcpPromptFinalization<'_>,
) -> Result<()> {
    let txn = db.begin().await?;
    let operation = async {
        let Some(message) = acp_messages::Entity::find_by_id(finalization.message_id)
            .one(&txn)
            .await?
        else {
            return Err(AQBotError::NotFound(format!(
                "ACP message {}",
                finalization.message_id
            )));
        };
        if message.thread_id != finalization.thread_id {
            return Err(AQBotError::Validation(format!(
                "ACP message {} does not belong to thread {}",
                finalization.message_id, finalization.thread_id
            )));
        }
        let mut message_update: acp_messages::ActiveModel = message.into();
        message_update.content = Set(finalization.content.to_string());
        message_update.status = Set(Some(finalization.message_status.to_string()));
        if let Some(meta_json) = finalization.meta_json {
            message_update.meta_json = Set(Some(meta_json.to_string()));
        }
        message_update.update(&txn).await?;

        let Some(thread) = acp_threads::Entity::find_by_id(finalization.thread_id)
            .one(&txn)
            .await?
        else {
            return Err(AQBotError::NotFound(format!(
                "ACP thread {}",
                finalization.thread_id
            )));
        };
        let another_prompt_is_running =
            thread_has_streaming_prompt(&txn, finalization.thread_id).await?;
        let mut thread_update: acp_threads::ActiveModel = thread.into();
        if let Some(session_id) = finalization.acp_session_id {
            thread_update.acp_session_id = Set(Some(session_id.to_string()));
        }
        thread_update.runtime_status = Set(if another_prompt_is_running {
            "running".to_string()
        } else {
            finalization.runtime_status.to_string()
        });
        thread_update.updated_at = Set(now_str());
        thread_update.update(&txn).await?;
        Ok(())
    }
    .await;
    if let Err(error) = operation {
        let rollback = txn.rollback().await.err();
        return Err(transaction_failure(error, rollback));
    }
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::stored_files;
    use base64::Engine;

    async fn thread_fixture(db: &DatabaseConnection) -> (acp_projects::Model, acp_threads::Model) {
        let project = create_project(db, "Project", "/tmp").await.unwrap();
        let thread = create_thread(db, &project.id, "agent", "Thread")
            .await
            .unwrap();
        (project, thread)
    }

    fn text_attachment(bytes: &[u8]) -> AttachmentInput {
        AttachmentInput {
            file_name: "notes.txt".to_string(),
            file_type: "text/plain".to_string(),
            file_size: bytes.len() as u64,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    #[tokio::test]
    async fn recent_workspace_is_distinct_from_user_projects() {
        let db = crate::db::create_test_pool().await.unwrap().conn;

        let project = create_project(&db, "Project", "/tmp/project")
            .await
            .unwrap();
        let recent = create_recent_workspace(&db, "Recent", "/tmp/recent")
            .await
            .unwrap();

        assert_eq!(project.kind, "project");
        assert_eq!(recent.kind, "recent");
    }

    #[tokio::test]
    async fn recent_draft_claim_is_atomic_and_cannot_be_reused() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let draft = create_recent_draft_workspace(&db, "New conversation", "/tmp/recent-draft")
            .await
            .unwrap();

        let thread = claim_recent_draft_thread(
            &db,
            &draft.id,
            "codex",
            "First prompt",
            Some("session-1"),
            Some("agent"),
        )
        .await
        .unwrap();
        let claimed = get_project(&db, &draft.id).await.unwrap().unwrap();

        assert_eq!(thread.project_id, draft.id);
        assert_eq!(thread.acp_session_id.as_deref(), Some("session-1"));
        assert_eq!(thread.mode_id.as_deref(), Some("agent"));
        assert_eq!(claimed.kind, "recent");
        assert_eq!(claimed.name, "First prompt");
        let duplicate =
            claim_recent_draft_thread(&db, &draft.id, "codex", "Second prompt", None, None)
                .await
                .unwrap_err();
        assert!(duplicate
            .to_string()
            .contains("no longer an unclaimed Recent draft"));
        assert_eq!(
            list_threads_for_project(&db, &draft.id)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn prompt_receipt_and_history_return_typed_attachment_metadata() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (_, thread) = thread_fixture(&db).await;
        let input = text_attachment(b"hello");

        let (user, assistant) =
            create_prompt_messages_using(&db, &store, &thread.id, "inspect", &[input.clone()])
                .await
                .unwrap();

        assert_eq!(user.attachments.len(), 1);
        assert!(user.attachments[0].data.is_none());
        assert!(assistant.attachments.is_empty());
        let history = list_messages(&db, &thread.id).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].attachments[0].id, user.attachments[0].id);
        let raw = acp_messages::Entity::find_by_id(&user.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .attachments_json
            .unwrap();
        assert!(!raw.contains(&input.data));
        let stored = crate::repo::stored_file::get_stored_file(&db, &user.attachments[0].id)
            .await
            .unwrap();
        assert!(stored.conversation_id.is_none());
    }

    #[tokio::test]
    async fn corrupt_attachment_json_fails_history_explicitly() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let (_, thread) = thread_fixture(&db).await;
        acp_messages::ActiveModel {
            id: Set("broken-message".to_string()),
            thread_id: Set(thread.id.clone()),
            role: Set("user".to_string()),
            content: Set(String::new()),
            status: Set(Some("done".to_string())),
            attachments_json: Set(Some("{not-json".to_string())),
            meta_json: Set(None),
            created_at: Set(now_str()),
        }
        .insert(&db)
        .await
        .unwrap();

        let error = list_messages(&db, &thread.id).await.unwrap_err();
        assert!(error.to_string().contains("broken-message"));
        assert!(error.to_string().contains("attachments JSON"));
    }

    #[tokio::test]
    async fn assistant_insert_failure_rolls_back_user_file_and_index_row() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (_, thread) = thread_fixture(&db).await;
        let bytes = b"rollback me";
        let input = text_attachment(bytes);
        let expected_path = crate::storage_paths::build_relative_path(
            &input.file_name,
            &input.file_type,
            &FileStore::hash_bytes(bytes),
        );
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TRIGGER fail_acp_assistant BEFORE INSERT ON acp_messages \
             WHEN NEW.role = 'assistant' BEGIN SELECT RAISE(ABORT, 'assistant failed'); END;",
        ))
        .await
        .unwrap();

        let result = create_prompt_messages_using(&db, &store, &thread.id, "go", &[input]).await;

        assert!(result.is_err());
        assert!(list_message_models(&db, &thread.id)
            .await
            .unwrap()
            .is_empty());
        assert!(stored_files::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .is_empty());
        assert!(!store.resolve_path(&expected_path).exists());
    }

    #[tokio::test]
    async fn duplicated_thread_shares_reference_until_last_thread_is_deleted() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (_, thread) = thread_fixture(&db).await;
        let (user, assistant) = create_prompt_messages_using(
            &db,
            &store,
            &thread.id,
            "share",
            &[text_attachment(b"shared")],
        )
        .await
        .unwrap();
        let attachment = user.attachments[0].clone();
        update_message_content(&db, &assistant.id, "done", Some("done"), None)
            .await
            .unwrap();
        let duplicate = duplicate_thread(&db, &thread.id, " copy")
            .await
            .unwrap()
            .unwrap();
        update_thread_session(&db, &thread.id, None, "idle")
            .await
            .unwrap();

        delete_thread_using(&db, &store, &thread.id).await.unwrap();

        assert!(stored_files::Entity::find_by_id(&attachment.id)
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(store.resolve_path(&attachment.file_path).exists());
        assert_eq!(
            list_messages(&db, &duplicate.id).await.unwrap()[0].attachments[0].id,
            attachment.id
        );

        delete_thread_using(&db, &store, &duplicate.id)
            .await
            .unwrap();
        assert!(stored_files::Entity::find_by_id(&attachment.id)
            .one(&db)
            .await
            .unwrap()
            .is_none());
        assert!(!store.resolve_path(&attachment.file_path).exists());
    }

    #[tokio::test]
    async fn pre_dispatch_rollback_removes_both_messages_and_attachment_storage() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (_, thread) = thread_fixture(&db).await;
        let (user, assistant) = create_prompt_messages_using(
            &db,
            &store,
            &thread.id,
            "will not dispatch",
            &[text_attachment(b"temporary")],
        )
        .await
        .unwrap();
        let attachment = user.attachments[0].clone();

        rollback_prompt_messages_using(&db, &store, &thread.id, &[user.id, assistant.id])
            .await
            .unwrap();

        assert!(list_message_models(&db, &thread.id)
            .await
            .unwrap()
            .is_empty());
        assert!(stored_files::Entity::find_by_id(&attachment.id)
            .one(&db)
            .await
            .unwrap()
            .is_none());
        assert!(!store.resolve_path(&attachment.file_path).exists());
        assert_eq!(
            get_thread(&db, &thread.id)
                .await
                .unwrap()
                .unwrap()
                .runtime_status,
            "idle"
        );
    }

    #[tokio::test]
    async fn session_discovery_does_not_overwrite_a_running_status() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let (_, thread) = thread_fixture(&db).await;
        create_prompt_messages(&db, &thread.id, "running", &[])
            .await
            .unwrap();

        update_thread_session_id(&db, &thread.id, "discovered-session")
            .await
            .unwrap();

        let thread = get_thread(&db, &thread.id).await.unwrap().unwrap();
        assert_eq!(thread.acp_session_id.as_deref(), Some("discovered-session"));
        assert_eq!(thread.runtime_status, "running");
    }

    #[tokio::test]
    async fn prepared_session_persistence_reports_a_concurrently_deleted_thread() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let (_, thread) = thread_fixture(&db).await;

        assert!(
            persist_prepared_thread_session(&db, &thread.id, "prepared-session", Some("plan"),)
                .await
                .unwrap()
        );
        let persisted = get_thread(&db, &thread.id).await.unwrap().unwrap();
        assert_eq!(
            persisted.acp_session_id.as_deref(),
            Some("prepared-session")
        );
        assert_eq!(persisted.mode_id.as_deref(), Some("plan"));

        delete_thread(&db, &thread.id).await.unwrap();
        assert!(
            !persist_prepared_thread_session(&db, &thread.id, "late-session", None,)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn rejecting_one_dispatch_does_not_idle_another_running_prompt() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (_, thread) = thread_fixture(&db).await;
        let (_, running_assistant) =
            create_prompt_messages_using(&db, &store, &thread.id, "first", &[])
                .await
                .unwrap();
        let rejected_user = insert_message(
            &db,
            &thread.id,
            "user",
            "rejected",
            Some("done"),
            None,
            None,
        )
        .await
        .unwrap();
        let rejected_assistant = insert_message(
            &db,
            &thread.id,
            "assistant",
            "",
            Some("streaming"),
            None,
            None,
        )
        .await
        .unwrap();

        rollback_prompt_messages_using(
            &db,
            &store,
            &thread.id,
            &[rejected_user.id, rejected_assistant.id],
        )
        .await
        .unwrap();

        assert_eq!(
            get_thread(&db, &thread.id)
                .await
                .unwrap()
                .unwrap()
                .runtime_status,
            "running"
        );
        assert!(acp_messages::Entity::find_by_id(running_assistant.id)
            .one(&db)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn a_second_prompt_receipt_is_rejected_while_one_is_streaming() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (_, thread) = thread_fixture(&db).await;
        create_prompt_messages_using(&db, &store, &thread.id, "first", &[])
            .await
            .unwrap();

        let error = create_prompt_messages_using(
            &db,
            &store,
            &thread.id,
            "second",
            &[text_attachment(b"must not persist")],
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("while one is running"));
        assert_eq!(list_message_models(&db, &thread.id).await.unwrap().len(), 2);
        assert!(stored_files::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn deleting_project_reclaims_its_last_attachment_reference() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (project, thread) = thread_fixture(&db).await;
        let (user, assistant) = create_prompt_messages_using(
            &db,
            &store,
            &thread.id,
            "delete project",
            &[text_attachment(b"project file")],
        )
        .await
        .unwrap();
        let attachment = user.attachments[0].clone();
        update_message_content(&db, &assistant.id, "done", Some("done"), None)
            .await
            .unwrap();
        update_thread_session(&db, &thread.id, None, "idle")
            .await
            .unwrap();

        delete_project_using(&db, &store, &project.id)
            .await
            .unwrap();

        assert!(acp_projects::Entity::find_by_id(&project.id)
            .one(&db)
            .await
            .unwrap()
            .is_none());
        assert!(stored_files::Entity::find_by_id(&attachment.id)
            .one(&db)
            .await
            .unwrap()
            .is_none());
        assert!(!store.resolve_path(&attachment.file_path).exists());
    }

    #[tokio::test]
    async fn running_thread_cannot_delete_its_prompt_attachment() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (_, thread) = thread_fixture(&db).await;
        let (user, _) = create_prompt_messages_using(
            &db,
            &store,
            &thread.id,
            "running",
            &[text_attachment(b"still in use")],
        )
        .await
        .unwrap();
        let attachment = user.attachments[0].clone();
        update_thread_session(&db, &thread.id, None, "idle")
            .await
            .unwrap();

        let error = delete_thread_using(&db, &store, &thread.id)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("prompt is running"));
        assert!(acp_threads::Entity::find_by_id(&thread.id)
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(stored_files::Entity::find_by_id(&attachment.id)
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(store.resolve_path(&attachment.file_path).exists());
    }

    #[tokio::test]
    async fn streaming_message_blocks_project_deletion_even_with_a_stale_idle_status() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let store = FileStore::with_root(root.path().to_path_buf());
        let (project, thread) = thread_fixture(&db).await;
        let (user, _) = create_prompt_messages_using(
            &db,
            &store,
            &thread.id,
            "running project turn",
            &[text_attachment(b"still in use by project")],
        )
        .await
        .unwrap();
        let attachment = user.attachments[0].clone();
        update_thread_session(&db, &thread.id, None, "idle")
            .await
            .unwrap();

        let error = delete_project_using(&db, &store, &project.id)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("prompts is running"));
        assert!(acp_projects::Entity::find_by_id(&project.id)
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(stored_files::Entity::find_by_id(&attachment.id)
            .one(&db)
            .await
            .unwrap()
            .is_some());
        assert!(store.resolve_path(&attachment.file_path).exists());
    }

    #[tokio::test]
    async fn stale_streaming_assistant_is_finalized_as_a_visible_error() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let (_, thread) = thread_fixture(&db).await;
        let (_, assistant) = create_prompt_messages(&db, &thread.id, "hello", &[])
            .await
            .unwrap();

        assert_eq!(
            interrupt_streaming_messages(&db, &thread.id, "Agent disconnected")
                .await
                .unwrap(),
            1
        );

        let history = list_messages(&db, &thread.id).await.unwrap();
        let assistant = history
            .iter()
            .find(|message| message.id == assistant.id)
            .unwrap();
        assert_eq!(assistant.status.as_deref(), Some("error"));
        assert!(assistant.content.contains("Agent disconnected"));
        assert_eq!(
            get_thread(&db, &thread.id)
                .await
                .unwrap()
                .unwrap()
                .runtime_status,
            "error"
        );
    }

    #[tokio::test]
    async fn finalization_rolls_back_the_message_when_the_thread_update_fails() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let (_, thread) = thread_fixture(&db).await;
        let (_, assistant) = create_prompt_messages(&db, &thread.id, "hello", &[])
            .await
            .unwrap();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TRIGGER fail_acp_finalize BEFORE UPDATE ON acp_threads \
             WHEN NEW.runtime_status = 'idle' BEGIN SELECT RAISE(ABORT, 'thread failed'); END;",
        ))
        .await
        .unwrap();

        let result = finalize_prompt(
            &db,
            AcpPromptFinalization {
                thread_id: &thread.id,
                message_id: &assistant.id,
                content: "completed",
                message_status: "done",
                meta_json: Some("{}"),
                acp_session_id: Some("session-final"),
                runtime_status: "idle",
            },
        )
        .await;

        assert!(result.is_err());
        let assistant = acp_messages::Entity::find_by_id(&assistant.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(assistant.status.as_deref(), Some("streaming"));
        assert!(assistant.content.is_empty());
        let thread = get_thread(&db, &thread.id).await.unwrap().unwrap();
        assert_eq!(thread.runtime_status, "running");
        assert!(thread.acp_session_id.is_none());
    }

    #[tokio::test]
    async fn startup_recovery_finalizes_streaming_turns_before_runtime_sessions_exist() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let (project, first_thread) = thread_fixture(&db).await;
        let second_thread = create_thread(&db, &project.id, "agent", "Second")
            .await
            .unwrap();
        create_prompt_messages(&db, &first_thread.id, "first", &[])
            .await
            .unwrap();
        create_prompt_messages(&db, &second_thread.id, "second", &[])
            .await
            .unwrap();

        let interrupted =
            interrupt_all_streaming_messages(&db, "The previous Agent turn was interrupted")
                .await
                .unwrap();

        assert_eq!(interrupted, 2);
        for thread_id in [&first_thread.id, &second_thread.id] {
            let thread = get_thread(&db, thread_id).await.unwrap().unwrap();
            assert_eq!(thread.runtime_status, "error");
            let assistant = list_message_models(&db, thread_id)
                .await
                .unwrap()
                .into_iter()
                .find(|message| message.role == "assistant")
                .unwrap();
            assert_eq!(assistant.status.as_deref(), Some("error"));
        }
    }
}
