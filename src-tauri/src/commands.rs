use crate::error::{AppError, AppResult};
use crate::import::parse_accounts;
use crate::models::*;
use crate::providers;
use crate::AppState;
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
pub fn unlock_app(state: State<'_, AppState>, password: String) -> AppResult<AppStatus> {
    let mut db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.unlock(&password)?;
    db.app_status()
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
pub fn list_tags(state: State<'_, AppState>) -> AppResult<Vec<Tag>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_tags()
}

#[tauri::command]
pub fn create_tag(state: State<'_, AppState>, input: CreateTagInput) -> AppResult<Tag> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_tag(input)
}

#[tauri::command]
pub fn delete_tag(state: State<'_, AppState>, tag_id: i64) -> AppResult<()> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_tag(tag_id)
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
pub fn exchange_oauth_token(state: State<'_, AppState>, input: OAuthExchangeInput) -> AppResult<OAuthTokenResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.exchange_oauth_token(input)
}

#[tauri::command]
pub fn download_attachment(state: State<'_, AppState>, input: DownloadAttachmentInput) -> AppResult<DownloadAttachmentResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.download_attachment(input)
}

#[tauri::command]
pub fn export_mail_messages(state: State<'_, AppState>, input: ExportMailMessagesInput) -> AppResult<ExportResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_mail_messages(input)
}

#[tauri::command]
pub fn export_accounts(state: State<'_, AppState>, input: Option<ExportAccountsInput>) -> AppResult<ExportResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_accounts(input.unwrap_or(ExportAccountsInput { group_id: None }))
}

#[tauri::command]
pub fn export_project_accounts(state: State<'_, AppState>, input: ExportProjectAccountsInput) -> AppResult<ExportResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_project_accounts(input)
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_projects()
}

#[tauri::command]
pub fn create_project(state: State<'_, AppState>, input: CreateProjectInput) -> AppResult<Project> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_project(input)
}

#[tauri::command]
pub fn get_project(state: State<'_, AppState>, project_id: i64) -> AppResult<Project> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.get_project(project_id)
}

#[tauri::command]
pub fn sync_project_scope(state: State<'_, AppState>, project_id: i64) -> AppResult<Project> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.sync_project_scope(project_id)
}

#[tauri::command]
pub fn list_project_accounts(state: State<'_, AppState>, project_id: i64) -> AppResult<Vec<ProjectAccount>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_project_accounts(project_id)
}

#[tauri::command]
pub fn claim_project_account(state: State<'_, AppState>, input: ClaimProjectAccountInput) -> AppResult<Option<ProjectAccount>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.claim_project_account(input)
}

#[tauri::command]
pub fn complete_project_account_success(state: State<'_, AppState>, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.complete_project_account_success(input)
}

#[tauri::command]
pub fn complete_project_account_failed(state: State<'_, AppState>, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.complete_project_account_failed(input)
}

#[tauri::command]
pub fn release_project_account(state: State<'_, AppState>, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.release_project_account(input)
}

#[tauri::command]
pub fn remove_project_account(state: State<'_, AppState>, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.remove_project_account(input)
}

#[tauri::command]
pub fn restore_project_account(state: State<'_, AppState>, input: ProjectAccountActionInput) -> AppResult<ProjectAccount> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.restore_project_account(input)
}

#[tauri::command]
pub fn list_project_events(state: State<'_, AppState>, project_id: i64) -> AppResult<Vec<ProjectAccountEvent>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_project_events(project_id)
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
pub fn run_forwarding_job(state: State<'_, AppState>, input: Option<ForwardingInput>) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.run_forwarding_job(input)
}

#[tauri::command]
pub fn run_backup_job(state: State<'_, AppState>) -> AppResult<BackupResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.run_backup_job()
}

#[tauri::command]
pub fn list_forwarding_logs(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<ForwardingLog>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_forwarding_logs(limit)
}

#[tauri::command]
pub fn list_backup_logs(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<BackupLog>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_backup_logs(limit)
}

#[tauri::command]
pub fn scheduler_status(state: State<'_, AppState>) -> AppResult<SchedulerStatus> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.scheduler_status()
}

#[tauri::command]
pub fn list_automation_runs(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<AutomationRun>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_automation_runs(limit)
}

#[tauri::command]
pub fn list_temp_emails(state: State<'_, AppState>) -> AppResult<Vec<TempEmail>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_temp_emails()
}

#[tauri::command]
pub fn import_temp_emails(state: State<'_, AppState>, input: ImportTempEmailsInput) -> AppResult<ImportAccountsResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.import_temp_emails(input)
}

#[tauri::command]
pub fn generate_temp_email(state: State<'_, AppState>, input: GenerateTempEmailInput) -> AppResult<TempEmail> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.generate_temp_email(input)
}

#[tauri::command]
pub fn delete_temp_email(state: State<'_, AppState>, input: TempEmailAddressInput) -> AppResult<()> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_temp_email(input.email)
}

#[tauri::command]
pub fn refresh_temp_email_messages(state: State<'_, AppState>, input: TempEmailAddressInput) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.refresh_temp_email_messages(input.email)
}

#[tauri::command]
pub fn list_temp_email_messages(state: State<'_, AppState>, email: String) -> AppResult<Vec<TempEmailMessage>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_temp_email_messages(email)
}

#[tauri::command]
pub fn list_cloudflare_channels(state: State<'_, AppState>) -> AppResult<Vec<CloudflareChannel>> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_cloudflare_channels()
}

#[tauri::command]
pub fn upsert_cloudflare_channel(state: State<'_, AppState>, input: UpsertCloudflareChannelInput) -> AppResult<CloudflareChannel> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.upsert_cloudflare_channel(input)
}

#[tauri::command]
pub fn delete_cloudflare_channel(state: State<'_, AppState>, channel_id: i64) -> AppResult<()> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_cloudflare_channel(channel_id)
}

#[tauri::command]
pub fn test_cloudflare_channel(state: State<'_, AppState>, channel_id: i64) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.test_cloudflare_channel(channel_id)
}

#[tauri::command]
pub fn run_refresh_job(state: State<'_, AppState>, input: Option<RefreshInput>, account_id: Option<i64>) -> AppResult<JobResult> {
    let db = state.db.lock().map_err(|err| AppError::Internal(err.to_string()))?;
    db.refresh_accounts(input.unwrap_or(RefreshInput {
        account_id,
        folder: Some("all".to_string()),
        top: Some(25),
    }))
}
