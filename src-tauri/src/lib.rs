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
            login_app,
            unlock_app,
            update_login_password,
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
            count_messages,
            create_demo_message,
            mark_mail_messages,
            delete_mail_messages,
            generate_oauth_auth_url,
            open_external_url,
            exchange_oauth_token,
            save_oauth_account,
            download_attachment,
            download_all_attachments,
            get_mail_raw_content,
            export_mail_messages,
            create_mail_share,
            list_mail_share_records,
            revoke_mail_share,
            export_accounts,
            export_account_secrets,
            get_settings,
            update_settings,
            get_local_retention_summary,
            clear_local_data,
            scheduler_status,
            list_workspace_key_records,
            generate_workspace_key,
            update_workspace_key_record,
            delete_workspace_key_record,
            run_refresh_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
