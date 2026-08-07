use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("acp_projects"))
                    .add_column(
                        ColumnDef::new(Alias::new("sort_order"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        // Backfill sequential sort_order by created_at so existing rows keep stable order
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            UPDATE acp_projects
            SET sort_order = (
              SELECT COUNT(*) FROM acp_projects AS p2
              WHERE p2.created_at < acp_projects.created_at
                 OR (p2.created_at = acp_projects.created_at AND p2.id <= acp_projects.id)
            ) - 1
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("acp_projects"))
                    .drop_column(Alias::new("sort_order"))
                    .to_owned(),
            )
            .await
    }
}
