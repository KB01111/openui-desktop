//! Tauri entry point: shared state, command registration, and the window.

pub mod agent;
pub mod commands;
pub mod contract;
pub mod openai;
pub mod settings;
pub mod sse;
pub mod store;
pub mod tools;

use tauri::Manager;

use commands::AppState;
use store::ThreadStore;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let store = ThreadStore::load(app.handle())?;
            app.manage(AppState::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat,
            commands::cancel_chat,
            commands::list_threads,
            commands::create_thread,
            commands::get_messages,
            commands::update_thread,
            commands::delete_thread,
            commands::get_settings,
            commands::save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
