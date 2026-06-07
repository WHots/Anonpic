#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;

use crate::core::base::configs::config_master::{
    apply_saved_ignore_self, generate_custom_data, load_config, save_config,
};
use crate::core::base::notify::notifications_handler;
use crate::core::base::screen_grab::free_roam_screen_grab::start_free_roam_capture;

fn main()
{
    std::thread::spawn(||
    {
        crate::core::logic::events::listener::listen();
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app|
        {
            notifications_handler::init(app.handle().clone());
            apply_saved_ignore_self(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            save_config,
            load_config,
            generate_custom_data,
            start_free_roam_capture
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
