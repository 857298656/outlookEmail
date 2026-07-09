use crate::error::{AppError, AppResult};
use crate::import::parse_accounts;
use crate::models::*;
use crate::providers;
use crate::AppState;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::Command;
use tauri::State;

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.app_status()
}

#[tauri::command]
pub fn initialize_app(state: State<'_, AppState>, password: String) -> AppResult<AppStatus> {
    let mut db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.initialize_app(&password)?;
    db.app_status()
}

#[tauri::command]
pub fn login_app(state: State<'_, AppState>, input: LoginInput) -> AppResult<AppStatus> {
    let mut db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.login(input)?;
    db.app_status()
}

#[tauri::command]
pub fn unlock_app(state: State<'_, AppState>, password: String) -> AppResult<AppStatus> {
    let mut db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.unlock(&password)?;
    db.app_status()
}

#[tauri::command]
pub fn update_login_password(
    state: State<'_, AppState>,
    input: UpdateLoginPasswordInput,
) -> AppResult<()> {
    let mut db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_login_password(input)
}

#[tauri::command]
pub fn lock_app(state: State<'_, AppState>) -> AppResult<AppStatus> {
    let mut db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.lock();
    db.app_status()
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<Group>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_groups()
}

#[tauri::command]
pub fn create_group(state: State<'_, AppState>, input: CreateGroupInput) -> AppResult<Group> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_group(input)
}

#[tauri::command]
pub fn update_group(state: State<'_, AppState>, input: UpdateGroupInput) -> AppResult<Group> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_group(input)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, group_id: i64) -> AppResult<()> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_group(group_id)
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> AppResult<Vec<Account>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_accounts()
}

#[tauri::command]
pub fn update_account(state: State<'_, AppState>, input: UpdateAccountInput) -> AppResult<Account> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_account(input)
}

#[tauri::command]
pub fn import_accounts(state: State<'_, AppState>, input: ImportAccountsInput) -> AppResult<ImportAccountsResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.import_accounts(parse_accounts(&input.raw), input.group_id)
}

#[tauri::command]
pub fn delete_account(state: State<'_, AppState>, account_id: i64) -> AppResult<()> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_account(account_id)
}

#[tauri::command]
pub fn batch_accounts(state: State<'_, AppState>, input: AccountBatchInput) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.batch_accounts(input)
}

#[tauri::command]
pub fn reveal_account_secrets(
    state: State<'_, AppState>,
    input: RevealAccountSecretsInput,
) -> AppResult<AccountSecretsPreview> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.reveal_account_secrets(input)
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    account_id: Option<i64>,
    folder: Option<String>,
    query: Option<MailMessageQuery>,
) -> AppResult<Vec<MailMessage>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    match query {
        Some(mut query) => {
            if query.account_id.is_none() {
                query.account_id = account_id;
            }
            if query.folder.is_none() {
                query.folder = folder;
            }
            db.list_messages_query(query)
        }
        None => db.list_messages(account_id, folder),
    }
}

#[tauri::command]
pub fn count_messages(
    state: State<'_, AppState>,
    account_id: Option<i64>,
    folder: Option<String>,
    query: Option<MailMessageQuery>,
) -> AppResult<i64> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    let mut query = query.unwrap_or_default();
    if query.account_id.is_none() {
        query.account_id = account_id;
    }
    if query.folder.is_none() {
        query.folder = folder;
    }
    db.count_messages_query(query)
}

#[tauri::command]
pub fn create_demo_message(state: State<'_, AppState>, account_id: i64) -> AppResult<MailMessage> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_demo_message(account_id)
}

#[tauri::command]
pub fn mark_mail_messages(state: State<'_, AppState>, input: MarkMailMessagesInput) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.mark_mail_messages(input)
}

#[tauri::command]
pub fn delete_mail_messages(state: State<'_, AppState>, input: DeleteMailMessagesInput) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_mail_messages(input)
}

#[tauri::command]
pub fn generate_oauth_auth_url(input: OAuthAuthUrlInput) -> AppResult<String> {
    providers::build_graph_auth_url(&input)
}

#[tauri::command]
pub fn open_external_url(url: String) -> AppResult<()> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err(AppError::InvalidInput("only http or https URLs can be opened".to_string()));
    }

    #[cfg(target_os = "windows")]
    let result = Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", trimmed])
        .spawn();

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(trimmed).spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(trimmed).spawn();

    result
        .map(|_| ())
        .map_err(|err| AppError::Internal(format!("failed to open default browser: {err}")))
}

#[tauri::command]
pub fn exchange_oauth_token(state: State<'_, AppState>, input: OAuthExchangeInput) -> AppResult<OAuthTokenResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.exchange_oauth_token(input)
}

#[tauri::command]
pub fn save_oauth_account(state: State<'_, AppState>, input: OAuthSaveAccountInput) -> AppResult<OAuthSaveAccountResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.save_oauth_account(input)
}

#[tauri::command]
pub fn download_attachment(state: State<'_, AppState>, input: DownloadAttachmentInput) -> AppResult<DownloadAttachmentResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.download_attachment(input)
}

#[tauri::command]
pub fn download_all_attachments(state: State<'_, AppState>, input: DownloadAllAttachmentsInput) -> AppResult<ExportResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.download_all_attachments(input)
}

#[tauri::command]
pub fn get_mail_raw_content(state: State<'_, AppState>, message_id: i64) -> AppResult<MailRawContent> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.get_mail_raw_content(message_id)
}

#[tauri::command]
pub fn export_mail_messages(state: State<'_, AppState>, input: ExportMailMessagesInput) -> AppResult<ExportResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_mail_messages(input)
}

#[tauri::command]
pub fn create_mail_share(state: State<'_, AppState>, input: CreateMailShareInput) -> AppResult<MailShareRecord> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_mail_share(input)
}

#[tauri::command]
pub fn list_mail_share_records(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<MailShareRecord>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_mail_share_records(limit)
}

#[tauri::command]
pub fn revoke_mail_share(state: State<'_, AppState>, input: RevokeMailShareInput) -> AppResult<MailShareRecord> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.revoke_mail_share(input)
}

#[tauri::command]
pub fn export_accounts(state: State<'_, AppState>, input: Option<ExportAccountsInput>) -> AppResult<ExportResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_accounts(input.unwrap_or(ExportAccountsInput {
        group_id: None,
        account_ids: None,
    }))
}

#[tauri::command]
pub fn export_account_secrets(state: State<'_, AppState>, input: ExportAccountSecretsInput) -> AppResult<ExportResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_account_secrets(input)
}














#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.get_settings()
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_settings(settings)
}




#[tauri::command]
pub fn get_local_retention_summary(state: State<'_, AppState>) -> AppResult<LocalRetentionSummary> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.local_retention_summary()
}

#[tauri::command]
pub fn clear_local_data(state: State<'_, AppState>, input: ClearLocalDataInput) -> AppResult<ClearLocalDataResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.clear_local_data(input)
}



#[tauri::command]
pub fn scheduler_status(state: State<'_, AppState>) -> AppResult<SchedulerStatus> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.scheduler_status()
}

#[tauri::command]
pub fn list_workspace_key_records(state: State<'_, AppState>) -> AppResult<Vec<WorkspaceKeyRecord>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_workspace_key_records()
}

#[tauri::command]
pub fn generate_workspace_key(
    state: State<'_, AppState>,
    input: GenerateWorkspaceKeyInput,
) -> AppResult<GenerateWorkspaceKeyResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.generate_workspace_key(input)
}

#[tauri::command]
pub fn update_workspace_key_record(
    state: State<'_, AppState>,
    input: UpdateWorkspaceKeyRecordInput,
) -> AppResult<WorkspaceKeyRecord> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_workspace_key_record(input)
}

#[tauri::command]
pub fn delete_workspace_key_record(state: State<'_, AppState>, record_id: i64) -> AppResult<()> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_workspace_key_record(record_id)
}

#[tauri::command]
pub fn run_refresh_job(state: State<'_, AppState>, input: Option<RefreshInput>, account_id: Option<i64>) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    let refresh_input = input.unwrap_or(RefreshInput {
        account_id,
        folder: Some("all".to_string()),
        top: None,
    });
    match catch_unwind(AssertUnwindSafe(|| db.refresh_accounts(refresh_input))) {
        Ok(result) => result,
        Err(payload) => Err(AppError::Internal(format!(
            "刷新任务异常中断：{}",
            panic_payload_message(payload.as_ref())
        ))),
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic".to_string()
}
