#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;

use crate::core::base::configs::config_master::{
    apply_saved_ignore_self, generate_custom_data, load_config, save_config,
};
use crate::core::base::notify::notifications_handler;
use crate::core::base::screen_grab::free_roam_screen_grab::start_free_roam_capture;
use crate::core::base::start_menu::start_menu_handler;
use crate::core::base::tray::tray_handler;

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
            start_menu_handler::apply_saved();
            tray_handler::init(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event|
        {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                let _ = window.hide();
                api.prevent_close();
            }
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
