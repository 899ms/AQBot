use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "acp_threads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub project_id: String,
    pub agent_id: String,
    pub title: String,
    pub acp_session_id: Option<String>,
    pub runtime_status: String,
    pub mode_id: Option<String>,
    /// 0 = unpinned, 1 = pinned (pinned threads sort first within a project)
    pub is_pinned: i32,
    /// Manual order within a project (after pin group)
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
