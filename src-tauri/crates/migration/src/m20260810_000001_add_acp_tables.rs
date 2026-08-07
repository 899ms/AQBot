use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Shared projects (not bound to a specific ACP agent)
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("acp_projects"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("name")).string().not_null())
                    .col(ColumnDef::new(Alias::new("root_path")).string().not_null())
                    .col(ColumnDef::new(Alias::new("created_at")).string().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).string().not_null())
                    .col(ColumnDef::new(Alias::new("last_opened_at")).string().null())
                    .to_owned(),
            )
            .await?;

        // Threads under a project; each binds one agent_id for life
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("acp_threads"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("project_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("agent_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("title")).string().not_null())
                    .col(ColumnDef::new(Alias::new("acp_session_id")).string().null())
                    .col(
                        ColumnDef::new(Alias::new("runtime_status"))
                            .string()
                            .not_null()
                            .default("idle"),
                    )
                    .col(ColumnDef::new(Alias::new("mode_id")).string().null())
                    .col(ColumnDef::new(Alias::new("created_at")).string().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).string().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_acp_threads_project_id")
                    .table(Alias::new("acp_threads"))
                    .col(Alias::new("project_id"))
                    .to_owned(),
            )
            .await?;

        // Independent message store (not shared with chat messages)
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("acp_messages"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("thread_id")).string().not_null())
                    .col(ColumnDef::new(Alias::new("role")).string().not_null())
                    .col(ColumnDef::new(Alias::new("content")).text().not_null())
                    .col(ColumnDef::new(Alias::new("status")).string().null())
                    .col(ColumnDef::new(Alias::new("attachments_json")).text().null())
                    .col(ColumnDef::new(Alias::new("meta_json")).text().null())
                    .col(ColumnDef::new(Alias::new("created_at")).string().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_acp_messages_thread_id")
                    .table(Alias::new("acp_messages"))
                    .col(Alias::new("thread_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("acp_messages")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("acp_threads")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("acp_projects")).to_owned())
            .await?;
        Ok(())
    }
}
