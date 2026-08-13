mod commands;
mod crypto;
mod db;
mod error;
mod import;
mod models;
mod providers;
mod temp_mail;

use commands::*;
use db::Database;
use std::sync::Mutex;

pub struct AppState {
    db: Mutex<Database>,
}

pub fn run() {
    let database = Database::open().expect("failed to initialize local database");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            db: Mutex::new(database),
        })
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
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
            update_group,
            delete_group,
            list_markdown_categories,
            create_markdown_category,
            update_markdown_category,
            delete_markdown_category,
            list_markdown_documents,
            get_markdown_document,
            create_markdown_document,
            update_markdown_document,
            delete_markdown_document,
            read_markdown_file,
            write_markdown_file,
            write_markdown_export_file,
            export_markdown_folder,
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
            list_workspace_key_records,
            generate_workspace_key,
            update_workspace_key_record,
            delete_workspace_key_record,
            list_temp_emails,
            generate_temp_email,
            import_temp_emails,
            generate_temp_emails_batch,
            list_temp_email_domains,
            list_cloudflare_channels,
            save_cloudflare_channel,
            delete_cloudflare_channel,
            list_temp_email_messages,
            refresh_temp_email_messages,
            get_temp_email_message,
            delete_temp_email,
            run_refresh_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
