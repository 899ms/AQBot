use base64::Engine;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, Set, TransactionTrait};

use crate::entity::stored_files;
use crate::error::{AQBotError, Result};
use crate::file_store::FileStore;
use crate::types::{Attachment, AttachmentInput};
use crate::utils::gen_id;

fn decode_inputs(inputs: &[AttachmentInput]) -> Result<Vec<Vec<u8>>> {
    inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            if crate::inline_media::contains_inline_image_data(&input.file_name)
                || crate::inline_media::contains_inline_image_data(&input.file_type)
            {
                return Err(AQBotError::Validation(format!(
                    "Attachment {index} metadata contains inline image data"
                )));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&input.data)
                .map_err(|error| {
                    AQBotError::Validation(format!(
                        "Invalid attachment base64 for {}: {error}",
                        input.file_name
                    ))
                })?;
            if bytes.len() as u64 != input.file_size {
                return Err(AQBotError::Validation(format!(
                    "Attachment size mismatch for {}: declared {}, decoded {}",
                    input.file_name,
                    input.file_size,
                    bytes.len()
                )));
            }
            Ok(bytes)
        })
        .collect()
}

/// Persist attachment bytes and `stored_files` rows using a caller-owned
/// transaction. The database stores only metadata and documents-root-relative
/// paths; inline Base64 is never copied into the returned attachments.
pub(crate) async fn persist_attachments_in_transaction<C>(
    db: &C,
    file_store: &FileStore,
    conversation_id: Option<&str>,
    inputs: &[AttachmentInput],
    created_paths: &mut Vec<String>,
) -> Result<Vec<Attachment>>
where
    C: ConnectionTrait,
{
    // Decode the complete batch first so malformed input cannot leave even a
    // temporary physical side effect.
    let decoded = decode_inputs(inputs)?;
    let mut attachments = Vec::with_capacity(inputs.len());

    for (input, bytes) in inputs.iter().zip(decoded) {
        let mime_type = crate::storage_paths::normalize_attachment_mime_type(
            &input.file_name,
            &input.file_type,
        );
        let saved = file_store.save_file(&bytes, &input.file_name, &mime_type)?;
        if saved.created {
            created_paths.push(saved.storage_path.clone());
        }
        let stored_file_id = gen_id();
        stored_files::ActiveModel {
            id: Set(stored_file_id.clone()),
            hash: Set(saved.hash),
            original_name: Set(input.file_name.clone()),
            mime_type: Set(mime_type.clone()),
            size_bytes: Set(saved.size_bytes),
            storage_path: Set(saved.storage_path.clone()),
            conversation_id: Set(conversation_id.map(str::to_string)),
            ..Default::default()
        }
        .insert(db)
        .await?;

        attachments.push(Attachment {
            id: stored_file_id,
            file_type: mime_type,
            file_name: input.file_name.clone(),
            file_path: saved.storage_path,
            file_size: saved.size_bytes as u64,
            data: None,
        });
    }

    Ok(attachments)
}

pub(crate) async fn cleanup_created_paths(
    db: &DatabaseConnection,
    file_store: &FileStore,
    paths: &[String],
) -> Vec<String> {
    let mut errors = Vec::new();
    for path in paths {
        match crate::repo::stored_file::count_stored_files_with_storage_path(db, path).await {
            Ok(0) => {
                if let Err(error) = file_store.delete_file(path) {
                    errors.push(format!("failed to remove {path}: {error}"));
                }
            }
            Ok(_) => {}
            Err(error) => errors.push(format!("failed to inspect {path}: {error}")),
        }
    }
    errors
}

fn persistence_failure(
    primary: AQBotError,
    rollback: Option<sea_orm::DbErr>,
    cleanup: Vec<String>,
) -> AQBotError {
    if rollback.is_none() && cleanup.is_empty() {
        return primary;
    }
    AQBotError::Validation(format!(
        "{primary}; rollback error: {}; cleanup errors: {}",
        rollback
            .map(|error| error.to_string())
            .unwrap_or_else(|| "none".to_string()),
        if cleanup.is_empty() {
            "none".to_string()
        } else {
            cleanup.join(", ")
        }
    ))
}

pub(crate) async fn persist_attachments_with_store(
    db: &DatabaseConnection,
    file_store: &FileStore,
    conversation_id: Option<&str>,
    inputs: &[AttachmentInput],
) -> Result<Vec<Attachment>> {
    let _file_reference_guard = crate::repo::stored_file::lock_file_references().await;
    let txn = db.begin().await?;
    let mut created_paths = Vec::new();
    let operation = persist_attachments_in_transaction(
        &txn,
        file_store,
        conversation_id,
        inputs,
        &mut created_paths,
    )
    .await;
    let attachments = match operation {
        Ok(attachments) => attachments,
        Err(error) => {
            let rollback = txn.rollback().await.err();
            let cleanup = cleanup_created_paths(db, file_store, &created_paths).await;
            return Err(persistence_failure(error, rollback, cleanup));
        }
    };
    if let Err(error) = txn.commit().await {
        let cleanup = cleanup_created_paths(db, file_store, &created_paths).await;
        return Err(persistence_failure(error.into(), None, cleanup));
    }
    Ok(attachments)
}

/// Persist a complete attachment batch under the active AQBot documents root.
pub async fn persist_attachments(
    db: &DatabaseConnection,
    conversation_id: Option<&str>,
    inputs: &[AttachmentInput],
) -> Result<Vec<Attachment>> {
    crate::storage_paths::ensure_documents_dirs()?;
    persist_attachments_with_store(db, &FileStore::new(), conversation_id, inputs).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::EntityTrait;

    #[tokio::test]
    async fn acp_attachment_rows_are_unowned_and_never_store_base64() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let file_store = FileStore::with_root(root.path().to_path_buf());
        let input = AttachmentInput {
            file_name: "screen shot.png".to_string(),
            file_type: "application/x-custom".to_string(),
            file_size: 3,
            data: base64::engine::general_purpose::STANDARD.encode(b"abc"),
        };

        let attachments = persist_attachments_with_store(&db, &file_store, None, &[input.clone()])
            .await
            .unwrap();

        assert_eq!(attachments.len(), 1);
        assert!(attachments[0].data.is_none());
        assert_eq!(attachments[0].file_type, "image/png");
        assert!(attachments[0].file_path.starts_with("images/"));
        assert!(!attachments[0].file_path.contains(&input.data));
        let row = stored_files::Entity::find_by_id(&attachments[0].id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(row.conversation_id.is_none());
        assert_eq!(row.mime_type, "image/png");
        assert!(!row.storage_path.contains(&input.data));
        assert_eq!(file_store.read_file(&row.storage_path).unwrap(), b"abc");
    }

    #[tokio::test]
    async fn malformed_batch_creates_neither_rows_nor_files() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let file_store = FileStore::with_root(root.path().to_path_buf());
        let bytes = b"unique attachment bytes";
        let valid = AttachmentInput {
            file_name: "first.txt".to_string(),
            file_type: "text/plain".to_string(),
            file_size: bytes.len() as u64,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        let invalid = AttachmentInput {
            file_name: "broken.txt".to_string(),
            file_type: "text/plain".to_string(),
            file_size: 1,
            data: "%%%not-base64%%%".to_string(),
        };
        let expected_path = crate::storage_paths::build_relative_path(
            &valid.file_name,
            &valid.file_type,
            &FileStore::hash_bytes(bytes),
        );

        let result =
            persist_attachments_with_store(&db, &file_store, None, &[valid, invalid]).await;

        assert!(result.is_err());
        assert!(stored_files::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .is_empty());
        assert!(!file_store.resolve_path(&expected_path).exists());
    }

    #[tokio::test]
    async fn declared_size_must_match_decoded_bytes() {
        let db = crate::db::create_test_pool().await.unwrap().conn;
        let root = tempfile::tempdir().unwrap();
        let file_store = FileStore::with_root(root.path().to_path_buf());
        let input = AttachmentInput {
            file_name: "wrong-size.bin".to_string(),
            file_type: "application/octet-stream".to_string(),
            file_size: 999,
            data: base64::engine::general_purpose::STANDARD.encode(b"abc"),
        };

        let error = persist_attachments_with_store(&db, &file_store, None, &[input])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("size mismatch"));
        assert!(stored_files::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .is_empty());
        assert!(std::fs::read_dir(root.path())
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(true));
    }
}
