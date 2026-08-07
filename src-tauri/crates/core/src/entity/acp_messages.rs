use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "acp_messages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub thread_id: String,
    pub role: String,
    #[sea_orm(column_type = "Text")]
    pub content: String,
    pub status: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub attachments_json: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub meta_json: Option<String>,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
