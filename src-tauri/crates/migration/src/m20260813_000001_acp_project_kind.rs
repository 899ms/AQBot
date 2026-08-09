use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_column("acp_projects", "kind").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("acp_projects"))
                        .add_column(
                            ColumnDef::new(Alias::new("kind"))
                                .string()
                                .not_null()
                                .default("project"),
                        )
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_column("acp_projects", "kind").await? {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("acp_projects"))
                        .drop_column(Alias::new("kind"))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
