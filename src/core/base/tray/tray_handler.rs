//! System tray integration: the tray icon, its menu (open, capture, quit), and
//! restoring the main window from the tray. Pairs with the close-to-tray
//! handling in `main`, which hides the window instead of exiting so the
//! Print Screen hotkey stays alive in the background.

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::core::base::screen_grab::free_roam_screen_grab;

const MENU_OPEN: &str = "open";
const MENU_CAPTURE: &str = "capture";
const MENU_QUIT: &str = "quit";

/// Builds the tray icon with its menu and wires up the menu and click
/// handlers. Left-clicking the icon reopens the main window; the menu offers
/// open, capture, and quit. Returns an error when the menu or icon cannot be
/// created.
pub fn init(app: &AppHandle) -> tauri::Result<()>
{
    let open = MenuItem::with_id(app, MENU_OPEN, "Open Anonpic", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, MENU_CAPTURE, "Capture region", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &capture, &separator, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main").tooltip("Anonpic").menu(&menu).show_menu_on_left_click(false).on_menu_event(on_menu_event).on_tray_icon_event(on_tray_icon_event);

    if let Some(icon) = app.default_window_icon()
    {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}


/// Dispatches a tray menu selection to its action.
fn on_menu_event(app: &AppHandle, event: MenuEvent)
{
    match event.id.as_ref()
    {
        MENU_OPEN => show_main_window(app),
        MENU_CAPTURE => free_roam_screen_grab::spawn_capture(),
        MENU_QUIT => app.exit(0),
        _ => {}
    }
}


/// Reopens the main window when the tray icon is left-clicked.
fn on_tray_icon_event(tray: &TrayIcon, event: TrayIconEvent)
{
    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event
    {
        show_main_window(tray.app_handle());
    }
}


/// Shows, restores, and focuses the main window.
fn show_main_window(app: &AppHandle)
{
    let window = match app.get_webview_window("main")
    {
        Some(window) => window,
        None =>
        {
            eprintln!("tray: main window not found");
            return;
        }
    };

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}
