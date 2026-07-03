mod automation;
mod commands;
mod crypto;
mod db;
mod error;
mod import;
mod models;
mod providers;
mod scheduler;

use commands::*;
use db::Database;
use std::sync::Mutex;

pub struct AppState {
    db: Mutex<Database>,
}

pub fn run() {
    let database = Database::open().expect("failed to initialize local database");

    tauri::Builder::default()
        .manage(AppState {
            db: Mutex::new(database),
        })
        .setup(|app| {
            scheduler::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            initialize_app,
            unlock_app,
            lock_app,
            list_groups,
            create_group,
            update_group_proxy,
            update_group,
            delete_group,
            list_tags,
            create_tag,
            delete_tag,
            list_accounts,
            update_account,
            import_accounts,
            delete_account,
            batch_accounts,
            reveal_account_secrets,
            list_messages,
            create_demo_message,
            mark_mail_messages,
            delete_mail_messages,
            generate_oauth_auth_url,
            exchange_oauth_token,
            download_attachment,
            download_all_attachments,
            get_mail_raw_content,
            export_mail_messages,
            create_mail_share,
            list_mail_share_records,
            revoke_mail_share,
            export_accounts,
            export_account_secrets,
            export_project_accounts,
            list_projects,
            create_project,
            get_project,
            sync_project_scope,
            list_project_accounts,
            claim_project_account,
            complete_project_account_success,
            complete_project_account_failed,
            release_project_account,
            remove_project_account,
            restore_project_account,
            list_project_events,
            get_settings,
            update_settings,
            run_forwarding_job,
            run_backup_job,
            restore_backup,
            get_local_retention_summary,
            clear_local_data,
            list_forwarding_logs,
            list_backup_logs,
            scheduler_status,
            get_automation_observability,
            list_refresh_logs,
            list_automation_runs,
            clear_automation_runs,
            list_retry_queue,
            run_retry_queue,
            dismiss_retry_item,
            list_temp_emails,
            import_temp_emails,
            generate_temp_email,
            generate_cloudflare_temp_emails,
            delete_temp_email,
            refresh_temp_email_messages,
            update_temp_email,
            list_temp_email_messages,
            list_cloudflare_channels,
            upsert_cloudflare_channel,
            delete_cloudflare_channel,
            test_cloudflare_channel,
            run_refresh_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
