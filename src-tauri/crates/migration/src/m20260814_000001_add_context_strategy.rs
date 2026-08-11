use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("conversations"))
                    .add_column(
                        ColumnDef::new(Alias::new("context_strategy_override"))
                            .text()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE conversations \
                 SET context_strategy_override = CASE \
                     WHEN context_compression <> 0 THEN 'smart_summary' \
                     ELSE 'raw_truncate' \
                 END",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("conversations"))
                    .drop_column(Alias::new("context_strategy_override"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
