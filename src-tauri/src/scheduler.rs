use crate::AppState;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub fn start(app_handle: AppHandle) {
    let _ = thread::Builder::new()
        .name("outlook-email-scheduler".to_string())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(60));
            let state = app_handle.state::<AppState>();
            let Ok(db) = state.db.lock() else {
                continue;
            };
            if db.is_unlocked() {
                let _ = db.run_due_scheduled_jobs();
            }
        });
}
