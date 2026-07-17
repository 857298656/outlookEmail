use crate::error::{AppError, AppResult};
use crate::import::parse_accounts;
use crate::models::*;
use crate::providers;
use crate::AppState;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::process::Command;
use tauri::State;

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.app_status()
}

#[tauri::command]
pub fn initialize_app(state: State<'_, AppState>, password: String) -> AppResult<AppStatus> {
    let mut db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.initialize_app(&password)?;
    db.app_status()
}

#[tauri::command]
pub fn login_app(state: State<'_, AppState>, input: LoginInput) -> AppResult<AppStatus> {
    let mut db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.login(input)?;
    db.app_status()
}

#[tauri::command]
pub fn unlock_app(state: State<'_, AppState>, password: String) -> AppResult<AppStatus> {
    let mut db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.unlock(&password)?;
    db.app_status()
}

#[tauri::command]
pub fn update_login_password(
    state: State<'_, AppState>,
    input: UpdateLoginPasswordInput,
) -> AppResult<()> {
    let mut db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_login_password(input)
}

#[tauri::command]
pub fn lock_app(state: State<'_, AppState>) -> AppResult<AppStatus> {
    let mut db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.lock();
    db.app_status()
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<Group>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_groups()
}

#[tauri::command]
pub fn create_group(state: State<'_, AppState>, input: CreateGroupInput) -> AppResult<Group> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_group(input)
}

#[tauri::command]
pub fn update_group(state: State<'_, AppState>, input: UpdateGroupInput) -> AppResult<Group> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_group(input)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, group_id: i64) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_group(group_id)
}

#[tauri::command]
pub fn list_markdown_categories(state: State<'_, AppState>) -> AppResult<Vec<MarkdownCategory>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_markdown_categories()
}

#[tauri::command]
pub fn create_markdown_category(
    state: State<'_, AppState>,
    input: CreateMarkdownCategoryInput,
) -> AppResult<MarkdownCategory> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_markdown_category(input)
}

#[tauri::command]
pub fn update_markdown_category(
    state: State<'_, AppState>,
    input: UpdateMarkdownCategoryInput,
) -> AppResult<MarkdownCategory> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_markdown_category(input)
}

#[tauri::command]
pub fn delete_markdown_category(state: State<'_, AppState>, category_id: i64) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_markdown_category(category_id)
}

#[tauri::command]
pub fn list_markdown_documents(
    state: State<'_, AppState>,
    category_id: Option<i64>,
    search: Option<String>,
) -> AppResult<Vec<MarkdownDocument>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_markdown_documents(category_id, search)
}

#[tauri::command]
pub fn get_markdown_document(
    state: State<'_, AppState>,
    document_id: i64,
) -> AppResult<MarkdownDocument> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.get_markdown_document(document_id)
}

#[tauri::command]
pub fn create_markdown_document(
    state: State<'_, AppState>,
    input: CreateMarkdownDocumentInput,
) -> AppResult<MarkdownDocument> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_markdown_document(input)
}

#[tauri::command]
pub fn update_markdown_document(
    state: State<'_, AppState>,
    input: UpdateMarkdownDocumentInput,
) -> AppResult<MarkdownDocument> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_markdown_document(input)
}

#[tauri::command]
pub fn delete_markdown_document(state: State<'_, AppState>, document_id: i64) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_markdown_document(document_id)
}

#[tauri::command]
pub fn read_markdown_file(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<MarkdownFileContent> {
    {
        let db = state
            .db
            .lock()
            .map_err(|err| AppError::Internal(err.to_string()))?;
        if !db.is_unlocked() {
            return Err(AppError::Unauthorized);
        }
    }
    let path = validate_markdown_file_path(&path)?;
    let metadata = fs::metadata(path).map_err(|err| AppError::Internal(err.to_string()))?;
    const MAX_MARKDOWN_BYTES: u64 = 25 * 1024 * 1024;
    if metadata.len() > MAX_MARKDOWN_BYTES {
        return Err(AppError::InvalidInput(
            "markdown file exceeds the 25 MB limit".to_string(),
        ));
    }
    let content = fs::read_to_string(path).map_err(|err| AppError::Internal(err.to_string()))?;
    let content = content
        .strip_prefix('\u{feff}')
        .unwrap_or(&content)
        .to_string();
    Ok(MarkdownFileContent {
        path: path.to_string_lossy().to_string(),
        file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("document.md")
            .to_string(),
        content,
        size: metadata.len() as i64,
    })
}

#[tauri::command]
pub fn write_markdown_file(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> AppResult<MarkdownFileWriteResult> {
    {
        let db = state
            .db
            .lock()
            .map_err(|err| AppError::Internal(err.to_string()))?;
        if !db.is_unlocked() {
            return Err(AppError::Unauthorized);
        }
    }
    let path = validate_markdown_file_path(&path)?;
    const MAX_MARKDOWN_BYTES: usize = 25 * 1024 * 1024;
    if content.len() > MAX_MARKDOWN_BYTES {
        return Err(AppError::InvalidInput(
            "markdown document exceeds the 25 MB limit".to_string(),
        ));
    }
    fs::write(path, content.as_bytes()).map_err(|err| AppError::Internal(err.to_string()))?;
    Ok(MarkdownFileWriteResult {
        path: path.to_string_lossy().to_string(),
        file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("document.md")
            .to_string(),
        size: content.len() as i64,
    })
}

#[tauri::command]
pub fn write_markdown_export_file(
    state: State<'_, AppState>,
    path: String,
    bytes: Vec<u8>,
) -> AppResult<MarkdownFileWriteResult> {
    {
        let db = state
            .db
            .lock()
            .map_err(|err| AppError::Internal(err.to_string()))?;
        if !db.is_unlocked() {
            return Err(AppError::Unauthorized);
        }
    }
    let path = Path::new(path.trim());
    if path.file_name().is_none() {
        return Err(AppError::InvalidInput(
            "markdown export path is required".to_string(),
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "html" | "pdf" | "png" | "jpg" | "jpeg") {
        return Err(AppError::InvalidInput(
            "markdown export must be HTML, PDF, PNG, or JPEG".to_string(),
        ));
    }
    const MAX_EXPORT_BYTES: usize = 50 * 1024 * 1024;
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err(AppError::InvalidInput(
            "markdown export exceeds the 50 MB limit".to_string(),
        ));
    }
    fs::write(path, &bytes).map_err(|err| AppError::Internal(err.to_string()))?;
    Ok(MarkdownFileWriteResult {
        path: path.to_string_lossy().to_string(),
        file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("markdown-export")
            .to_string(),
        size: bytes.len() as i64,
    })
}

#[tauri::command]
pub fn export_markdown_folder(
    state: State<'_, AppState>,
    category_id: i64,
    directory: String,
) -> AppResult<ExportResult> {
    let (categories, documents) = {
        let db = state
            .db
            .lock()
            .map_err(|err| AppError::Internal(err.to_string()))?;
        (
            db.list_markdown_categories()?,
            db.list_markdown_documents(None, None)?,
        )
    };
    let category_by_id: HashMap<i64, MarkdownCategory> = categories
        .into_iter()
        .map(|category| (category.id, category))
        .collect();
    let root_category = category_by_id
        .get(&category_id)
        .ok_or_else(|| AppError::InvalidInput("markdown folder not found".to_string()))?;
    let mut descendant_ids = HashSet::from([category_id]);
    loop {
        let previous_len = descendant_ids.len();
        category_by_id.values().for_each(|category| {
            if category
                .parent_id
                .is_some_and(|parent_id| descendant_ids.contains(&parent_id))
            {
                descendant_ids.insert(category.id);
            }
        });
        if descendant_ids.len() == previous_len {
            break;
        }
    }

    let destination = Path::new(directory.trim());
    if !destination.is_dir() {
        return Err(AppError::InvalidInput(
            "markdown export directory not found".to_string(),
        ));
    }
    let root_name = safe_markdown_file_name(&root_category.name);
    let root_path = destination.join(&root_name);
    fs::create_dir_all(&root_path).map_err(|err| AppError::Internal(err.to_string()))?;

    for category in category_by_id
        .values()
        .filter(|category| descendant_ids.contains(&category.id))
    {
        let relative = markdown_category_relative_path(category.id, category_id, &category_by_id)?;
        fs::create_dir_all(root_path.join(relative))
            .map_err(|err| AppError::Internal(err.to_string()))?;
    }

    let mut item_count = 0usize;
    let mut total_size = 0i64;
    for document in documents.into_iter().filter(|document| {
        document
            .category_id
            .is_some_and(|category_id| descendant_ids.contains(&category_id))
    }) {
        let document_category_id = document.category_id.unwrap_or(category_id);
        let relative =
            markdown_category_relative_path(document_category_id, category_id, &category_by_id)?;
        let directory = root_path.join(relative);
        let base_name = safe_markdown_file_name(&document.title);
        let mut file_path = directory.join(format!("{base_name}.md"));
        if file_path.exists() {
            file_path = directory.join(format!("{base_name} ({}).md", document.id));
        }
        fs::write(&file_path, document.content.as_bytes())
            .map_err(|err| AppError::Internal(err.to_string()))?;
        item_count += 1;
        total_size += document.content.len() as i64;
    }

    Ok(ExportResult {
        path: root_path.to_string_lossy().to_string(),
        file_name: root_name,
        size: total_size,
        item_count,
    })
}

fn validate_markdown_file_path(value: &str) -> AppResult<&Path> {
    let path = Path::new(value);
    if value.trim().is_empty() || path.file_name().is_none() {
        return Err(AppError::InvalidInput(
            "markdown file path is required".to_string(),
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "md" && extension != "markdown" {
        return Err(AppError::InvalidInput(
            "only .md and .markdown files are supported".to_string(),
        ));
    }
    Ok(path)
}

fn safe_markdown_file_name(value: &str) -> String {
    let value = value
        .trim()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '-'
            } else {
                character
            }
        })
        .collect::<String>();
    let value = value.trim_end_matches(['.', ' ']).trim();
    if value.is_empty() {
        "document".to_string()
    } else {
        value.to_string()
    }
}

fn markdown_category_relative_path(
    category_id: i64,
    root_id: i64,
    categories: &HashMap<i64, MarkdownCategory>,
) -> AppResult<std::path::PathBuf> {
    let mut current_id = category_id;
    let mut segments = Vec::new();
    let mut visited = HashSet::new();
    while current_id != root_id {
        if !visited.insert(current_id) {
            return Err(AppError::Internal(
                "markdown folder tree contains a cycle".to_string(),
            ));
        }
        let category = categories
            .get(&current_id)
            .ok_or_else(|| AppError::Internal("markdown folder tree is incomplete".to_string()))?;
        segments.push(safe_markdown_file_name(&category.name));
        current_id = category.parent_id.ok_or_else(|| {
            AppError::Internal("markdown folder is outside the export tree".to_string())
        })?;
    }
    segments.reverse();
    Ok(segments.into_iter().collect())
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> AppResult<Vec<Account>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_accounts()
}

#[tauri::command]
pub fn update_account(state: State<'_, AppState>, input: UpdateAccountInput) -> AppResult<Account> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_account(input)
}

#[tauri::command]
pub fn import_accounts(
    state: State<'_, AppState>,
    input: ImportAccountsInput,
) -> AppResult<ImportAccountsResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.import_accounts(parse_accounts(&input.raw), input.group_id)
}

#[tauri::command]
pub fn delete_account(state: State<'_, AppState>, account_id: i64) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_account(account_id)
}

#[tauri::command]
pub fn batch_accounts(
    state: State<'_, AppState>,
    input: AccountBatchInput,
) -> AppResult<JobResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.batch_accounts(input)
}

#[tauri::command]
pub fn reveal_account_secrets(
    state: State<'_, AppState>,
    input: RevealAccountSecretsInput,
) -> AppResult<AccountSecretsPreview> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.reveal_account_secrets(input)
}

#[tauri::command]
pub fn list_temp_emails(state: State<'_, AppState>) -> AppResult<Vec<TempEmail>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_temp_emails()
}

#[tauri::command]
pub fn generate_temp_email(
    state: State<'_, AppState>,
    input: GenerateTempEmailInput,
) -> AppResult<TempEmail> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.generate_temp_email(input)
}

#[tauri::command]
pub fn import_temp_emails(
    state: State<'_, AppState>,
    input: ImportTempEmailsInput,
) -> AppResult<ImportTempEmailsResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.import_temp_emails(input)
}

#[tauri::command]
pub fn generate_temp_emails_batch(
    state: State<'_, AppState>,
    input: GenerateTempEmailsBatchInput,
) -> AppResult<GenerateTempEmailsBatchResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.generate_temp_emails_batch(input)
}

#[tauri::command]
pub fn list_temp_email_domains(
    state: State<'_, AppState>,
    input: TempEmailProviderConfig,
) -> AppResult<Vec<String>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_temp_email_domains(input)
}

#[tauri::command]
pub fn list_cloudflare_channels(state: State<'_, AppState>) -> AppResult<Vec<CloudflareChannel>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_cloudflare_channels()
}

#[tauri::command]
pub fn save_cloudflare_channel(
    state: State<'_, AppState>,
    input: SaveCloudflareChannelInput,
) -> AppResult<CloudflareChannel> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.save_cloudflare_channel(input)
}

#[tauri::command]
pub fn delete_cloudflare_channel(state: State<'_, AppState>, channel_id: i64) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_cloudflare_channel(channel_id)
}

#[tauri::command]
pub fn list_temp_email_messages(
    state: State<'_, AppState>,
    temp_email_id: i64,
) -> AppResult<Vec<TempEmailMessage>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_temp_email_messages(temp_email_id)
}

#[tauri::command]
pub fn refresh_temp_email_messages(
    state: State<'_, AppState>,
    temp_email_id: i64,
) -> AppResult<Vec<TempEmailMessage>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.refresh_temp_email_messages(temp_email_id)
}

#[tauri::command]
pub fn get_temp_email_message(
    state: State<'_, AppState>,
    temp_email_id: i64,
    message_id: String,
) -> AppResult<TempEmailMessage> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.get_temp_email_message(temp_email_id, &message_id)
}

#[tauri::command]
pub fn delete_temp_email(state: State<'_, AppState>, temp_email_id: i64) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_temp_email(temp_email_id)
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    account_id: Option<i64>,
    folder: Option<String>,
    query: Option<MailMessageQuery>,
) -> AppResult<Vec<MailMessage>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
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
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
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
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_demo_message(account_id)
}

#[tauri::command]
pub fn mark_mail_messages(
    state: State<'_, AppState>,
    input: MarkMailMessagesInput,
) -> AppResult<JobResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.mark_mail_messages(input)
}

#[tauri::command]
pub fn delete_mail_messages(
    state: State<'_, AppState>,
    input: DeleteMailMessagesInput,
) -> AppResult<JobResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
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
        return Err(AppError::InvalidInput(
            "only http or https URLs can be opened".to_string(),
        ));
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
pub fn exchange_oauth_token(
    state: State<'_, AppState>,
    input: OAuthExchangeInput,
) -> AppResult<OAuthTokenResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.exchange_oauth_token(input)
}

#[tauri::command]
pub fn save_oauth_account(
    state: State<'_, AppState>,
    input: OAuthSaveAccountInput,
) -> AppResult<OAuthSaveAccountResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.save_oauth_account(input)
}

#[tauri::command]
pub fn download_attachment(
    state: State<'_, AppState>,
    input: DownloadAttachmentInput,
) -> AppResult<DownloadAttachmentResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.download_attachment(input)
}

#[tauri::command]
pub fn download_all_attachments(
    state: State<'_, AppState>,
    input: DownloadAllAttachmentsInput,
) -> AppResult<ExportResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.download_all_attachments(input)
}

#[tauri::command]
pub fn get_mail_raw_content(
    state: State<'_, AppState>,
    message_id: i64,
) -> AppResult<MailRawContent> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.get_mail_raw_content(message_id)
}

#[tauri::command]
pub fn export_mail_messages(
    state: State<'_, AppState>,
    input: ExportMailMessagesInput,
) -> AppResult<ExportResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_mail_messages(input)
}

#[tauri::command]
pub fn create_mail_share(
    state: State<'_, AppState>,
    input: CreateMailShareInput,
) -> AppResult<MailShareRecord> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.create_mail_share(input)
}

#[tauri::command]
pub fn list_mail_share_records(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> AppResult<Vec<MailShareRecord>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_mail_share_records(limit)
}

#[tauri::command]
pub fn revoke_mail_share(
    state: State<'_, AppState>,
    input: RevokeMailShareInput,
) -> AppResult<MailShareRecord> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.revoke_mail_share(input)
}

#[tauri::command]
pub fn export_accounts(
    state: State<'_, AppState>,
    input: Option<ExportAccountsInput>,
) -> AppResult<ExportResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_accounts(input.unwrap_or(ExportAccountsInput {
        group_id: None,
        account_ids: None,
    }))
}

#[tauri::command]
pub fn export_account_secrets(
    state: State<'_, AppState>,
    input: ExportAccountSecretsInput,
) -> AppResult<ExportResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.export_account_secrets(input)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.get_settings()
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_settings(settings)
}

#[tauri::command]
pub fn get_local_retention_summary(state: State<'_, AppState>) -> AppResult<LocalRetentionSummary> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.local_retention_summary()
}

#[tauri::command]
pub fn clear_local_data(
    state: State<'_, AppState>,
    input: ClearLocalDataInput,
) -> AppResult<ClearLocalDataResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.clear_local_data(input)
}

#[tauri::command]
pub fn scheduler_status(state: State<'_, AppState>) -> AppResult<SchedulerStatus> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.scheduler_status()
}

#[tauri::command]
pub fn list_workspace_key_records(
    state: State<'_, AppState>,
) -> AppResult<Vec<WorkspaceKeyRecord>> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.list_workspace_key_records()
}

#[tauri::command]
pub fn generate_workspace_key(
    state: State<'_, AppState>,
    input: GenerateWorkspaceKeyInput,
) -> AppResult<GenerateWorkspaceKeyResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.generate_workspace_key(input)
}

#[tauri::command]
pub fn update_workspace_key_record(
    state: State<'_, AppState>,
    input: UpdateWorkspaceKeyRecordInput,
) -> AppResult<WorkspaceKeyRecord> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.update_workspace_key_record(input)
}

#[tauri::command]
pub fn delete_workspace_key_record(state: State<'_, AppState>, record_id: i64) -> AppResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    db.delete_workspace_key_record(record_id)
}

#[tauri::command]
pub fn run_refresh_job(
    state: State<'_, AppState>,
    input: Option<RefreshInput>,
    account_id: Option<i64>,
) -> AppResult<JobResult> {
    let db = state
        .db
        .lock()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    let refresh_input = input.unwrap_or(RefreshInput {
        account_id,
        folder: Some("inbox_junk".to_string()),
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
