use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Conversations::Table)
                    .add_column(
                        ColumnDef::new(Conversations::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            UPDATE conversations
            SET sort_order = (
              SELECT COUNT(*)
              FROM conversations AS c2
              WHERE c2.category_id = conversations.category_id
                AND (
                  c2.updated_at > conversations.updated_at
                  OR (
                    c2.updated_at = conversations.updated_at
                    AND c2.id < conversations.id
                  )
                )
            )
            WHERE category_id IS NOT NULL
            "#,
        )
        .await?;
        db.execute_unprepared(
            r#"
            UPDATE conversations
            SET sort_order = (
              SELECT COUNT(*)
              FROM conversations AS c2
              WHERE c2.category_id IS NULL
                AND (
                  c2.is_pinned > conversations.is_pinned
                  OR (
                    c2.is_pinned = conversations.is_pinned
                    AND (
                      c2.updated_at > conversations.updated_at
                      OR (
                        c2.updated_at = conversations.updated_at
                        AND c2.id < conversations.id
                      )
                    )
                  )
                )
            )
            WHERE category_id IS NULL
            "#,
        )
        .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_conversations_category_active_root_sort")
                    .table(Conversations::Table)
                    .col(Conversations::CategoryId)
                    .col(Conversations::IsArchived)
                    .col(Conversations::ParentConversationId)
                    .col(Conversations::SortOrder)
                    .if_not_exists()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_conversations_category_active_root_sort")
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Conversations::Table)
                    .drop_column(Conversations::SortOrder)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Conversations {
    Table,
    CategoryId,
    IsArchived,
    ParentConversationId,
    SortOrder,
}
