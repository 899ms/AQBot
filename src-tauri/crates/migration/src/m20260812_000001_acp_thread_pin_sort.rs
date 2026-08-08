use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_column("acp_threads", "is_pinned").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("acp_threads"))
                        .add_column(
                            ColumnDef::new(Alias::new("is_pinned"))
                                .integer()
                                .not_null()
                                .default(0),
                        )
                        .to_owned(),
                )
                .await?;
        }

        if !manager.has_column("acp_threads", "sort_order").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("acp_threads"))
                        .add_column(
                            ColumnDef::new(Alias::new("sort_order"))
                                .integer()
                                .not_null()
                                .default(0),
                        )
                        .to_owned(),
                )
                .await?;
        }

        // Backfill sequential sort_order per project by updated_at (newest first → lower index)
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            UPDATE acp_threads
            SET sort_order = (
              SELECT COUNT(*) FROM acp_threads AS t2
              WHERE t2.project_id = acp_threads.project_id
                AND (
                  t2.updated_at > acp_threads.updated_at
                  OR (t2.updated_at = acp_threads.updated_at AND t2.id < acp_threads.id)
                )
            )
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_column("acp_threads", "sort_order").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("acp_threads"))
                        .drop_column(Alias::new("sort_order"))
                        .to_owned(),
                )
                .await?;
        }
        if manager.has_column("acp_threads", "is_pinned").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("acp_threads"))
                        .drop_column(Alias::new("is_pinned"))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
