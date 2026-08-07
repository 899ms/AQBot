use crate::entity::{acp_messages, acp_projects, acp_threads};
use crate::error::Result;
use crate::utils::gen_id;
use sea_orm::*;

fn now_str() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
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

pub async fn get_project(
    db: &DatabaseConnection,
    id: &str,
) -> Result<Option<acp_projects::Model>> {
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
    // Cascade: messages -> threads -> project
    let threads = list_threads_for_project(db, id).await?;
    for t in threads {
        delete_thread(db, &t.id).await?;
    }
    acp_projects::Entity::delete_by_id(id.to_string())
        .exec(db)
        .await?;
    Ok(())
}

// --- Threads ---

pub async fn list_threads_for_project(
    db: &DatabaseConnection,
    project_id: &str,
) -> Result<Vec<acp_threads::Model>> {
    Ok(acp_threads::Entity::find()
        .filter(acp_threads::Column::ProjectId.eq(project_id))
        .order_by_desc(acp_threads::Column::UpdatedAt)
        .all(db)
        .await?)
}

pub async fn list_all_threads(db: &DatabaseConnection) -> Result<Vec<acp_threads::Model>> {
    Ok(acp_threads::Entity::find()
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
    let now = now_str();
    let model = acp_threads::ActiveModel {
        id: Set(gen_id()),
        project_id: Set(project_id.to_string()),
        agent_id: Set(agent_id.to_string()),
        title: Set(title.to_string()),
        acp_session_id: Set(None),
        runtime_status: Set("idle".into()),
        mode_id: Set(None),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    };
    Ok(model.insert(db).await?)
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

pub async fn update_thread_title(db: &DatabaseConnection, id: &str, title: &str) -> Result<()> {
    if let Some(model) = get_thread(db, id).await? {
        let mut am: acp_threads::ActiveModel = model.into();
        am.title = Set(title.to_string());
        am.updated_at = Set(now_str());
        am.update(db).await?;
    }
    Ok(())
}

pub async fn delete_thread(db: &DatabaseConnection, id: &str) -> Result<()> {
    acp_messages::Entity::delete_many()
        .filter(acp_messages::Column::ThreadId.eq(id))
        .exec(db)
        .await?;
    acp_threads::Entity::delete_by_id(id.to_string())
        .exec(db)
        .await?;
    Ok(())
}

// --- Messages ---

pub async fn list_messages(
    db: &DatabaseConnection,
    thread_id: &str,
) -> Result<Vec<acp_messages::Model>> {
    Ok(acp_messages::Entity::find()
        .filter(acp_messages::Column::ThreadId.eq(thread_id))
        .order_by_asc(acp_messages::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn create_message(
    db: &DatabaseConnection,
    thread_id: &str,
    role: &str,
    content: &str,
    status: Option<&str>,
    meta_json: Option<&str>,
) -> Result<acp_messages::Model> {
    let model = acp_messages::ActiveModel {
        id: Set(gen_id()),
        thread_id: Set(thread_id.to_string()),
        role: Set(role.to_string()),
        content: Set(content.to_string()),
        status: Set(status.map(|s| s.to_string())),
        attachments_json: Set(None),
        meta_json: Set(meta_json.map(|s| s.to_string())),
        created_at: Set(now_str()),
    };
    let inserted = model.insert(db).await?;
    // bump thread updated_at
    if let Some(t) = get_thread(db, thread_id).await? {
        let mut am: acp_threads::ActiveModel = t.into();
        am.updated_at = Set(now_str());
        am.update(db).await?;
    }
    Ok(inserted)
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
