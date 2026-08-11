// Message version commands.

#[tauri::command]
pub async fn list_message_versions(
    state: State<'_, AppState>,
    conversation_id: String,
    parent_message_id: String,
) -> Result<Vec<Message>, String> {
    let messages = aqbot_core::repo::message::list_message_versions(
        &state.sea_db,
        &conversation_id,
        &parent_message_id,
    )
    .await
    .map_err(|e| e.to_string())?;
    let messages =
        crate::commands::messages::materialize_messages_for_ipc(&state.sea_db, messages).await?;
    Ok(messages)
}

#[tauri::command]
pub async fn list_message_versions_batch(
    state: State<'_, AppState>,
    conversation_id: String,
    parent_message_ids: Vec<String>,
) -> Result<HashMap<String, Vec<Message>>, String> {
    let mut versions = aqbot_core::repo::message::list_message_versions_batch(
        &state.sea_db,
        &conversation_id,
        &parent_message_ids,
    )
    .await
    .map_err(|e| e.to_string())?;
    for messages in versions.values_mut() {
        *messages = crate::commands::messages::materialize_messages_for_ipc(
            &state.sea_db,
            std::mem::take(messages),
        )
        .await?;
    }
    Ok(versions)
}

#[tauri::command]
pub async fn switch_message_version(
    state: State<'_, AppState>,
    conversation_id: String,
    parent_message_id: String,
    message_id: String,
) -> Result<(), String> {
    aqbot_core::repo::message::set_active_version(
        &state.sea_db,
        &conversation_id,
        &parent_message_id,
        &message_id,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_message_group(
    state: State<'_, AppState>,
    conversation_id: String,
    user_message_id: String,
) -> Result<(), String> {
    let file_store = aqbot_core::file_store::FileStore::new();
    let deleted = crate::commands::messages::delete_message_group_with_media_cleanup(
        &state.sea_db,
        &file_store,
        &user_message_id,
    )
    .await?;
    // Decrement message count by deleted count
    for _ in 0..deleted {
        aqbot_core::repo::conversation::decrement_message_count(&state.sea_db, &conversation_id)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
