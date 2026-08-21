//! Application configuration: the settings model and saving it to the working
//! directory's `config/app.cfg`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::core::base::start_menu::start_menu_handler;
use crate::core::helpers::file_operations::file_helper;

const CONFIG_DIR: &str = "config";
const CONFIG_FILE: &str = "app.cfg";
const CUSTOM_DATA_ENTRY_COUNT: usize = 2;

/// Persisted application settings.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config
{
    pub save_directory: String,
    pub image_format: String,
    /// Auto-save the cleaned capture to the save directory. Defaults to `true`
    /// so configs written before this option existed keep the prior behavior.
    #[serde(default = "default_true")]
    pub auto_save: bool,
    /// Copy the cleaned capture to the clipboard after it is grabbed.
    #[serde(default)]
    pub copy_to_clipboard: bool,
    /// Draw the free-roam region selection as a circle instead of a rectangle.
    #[serde(default)]
    pub circular_selection: bool,
    /// Show and capture the same frozen virtual-desktop frame during selection.
    /// Defaults to `true` so older configs preserve pre-capture frame timing.
    #[serde(default = "default_true")]
    pub freeze_screen_on_capture: bool,
    /// Prevent Anonpic's own window from appearing in screenshots.
    #[serde(default)]
    pub ignore_self: bool,
    /// Replace stripped image data with user-configured values after cleaning.
    #[serde(default)]
    pub fill_custom_data: bool,
    /// Keep a shortcut in the Start Menu so Anonpic shows up in Windows
    /// search. Enabled by default.
    #[serde(default = "default_true")]
    pub start_menu_shortcut: bool,
    /// User-configured replacement values for image data families.
    #[serde(default)]
    pub custom_data: CustomDataConfig,
}

/// Replacement values for image data families.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CustomDataConfig
{
    pub exif: String,
    pub metadata: String,
}


/// Saves the UI's settings as JSON to `<working_dir>/config/app.cfg`, creating
/// the config directory if needed. Returns `true` on success.
#[tauri::command]
pub fn save_config(app: tauri::AppHandle, config: Config) -> bool
{
    let saved = persist_config(&config);
    if saved
    {
        apply_ignore_self(&app, config.ignore_self);
        start_menu_handler::apply(config.start_menu_shortcut);
    }

    saved
}


/// Loads the persisted settings from `<working_dir>/config/app.cfg`, or `None`
/// when the file is absent or cannot be parsed.
#[tauri::command]
pub fn load_config() -> Option<Config>
{
    let path = config_dir()?.join(CONFIG_FILE);
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}


/// Generates unique replacement values for every custom data entry.
#[tauri::command]
pub fn generate_custom_data() -> CustomDataConfig
{
    let values = unique_random_strings(CUSTOM_DATA_ENTRY_COUNT);

    CustomDataConfig
    {
        exif: values[0].clone(),
        metadata: values[1].clone(),
    }
}


/// Applies the saved self-capture setting to the main window.
pub fn apply_saved_ignore_self(app: &tauri::AppHandle)
{
    let ignore_self = load_config().map(|config| config.ignore_self).unwrap_or(false);
    apply_ignore_self(app, ignore_self);
}


/// Returns `<working_dir>/config`, or `None` if the working directory cannot be
/// determined.
pub fn config_dir() -> Option<PathBuf>
{
    let mut dir = std::env::current_dir().ok()?;
    dir.push(CONFIG_DIR);
    Some(dir)
}


/// Serde default for `auto_save`, preserving the app's original always-save
/// behavior for older config files that lack the field.
fn default_true() -> bool
{
    true
}


/// Writes `config` to the config file, ensuring its directory exists first.
/// Returns `true` on success.
fn persist_config(config: &Config) -> bool
{
    let dir = match config_dir()
    {
        Some(dir) => dir,
        None =>
        {
            eprintln!("config: failed to resolve the config directory");
            return false;
        }
    };

    let dir = dir.to_string_lossy().into_owned();
    if !file_helper::create_directory(&dir)
    {
        eprintln!("config: failed to create config directory: {dir}");
        return false;
    }

    let json = match serde_json::to_string_pretty(config)
    {
        Ok(json) => json,
        Err(_) =>
        {
            eprintln!("config: failed to serialize settings");
            return false;
        }
    };

    let path = Path::new(&dir).join(CONFIG_FILE);
    let written = std::fs::write(&path, json).is_ok();
    if !written
    {
        eprintln!("config: failed to write {}", path.display());
    }

    written
}


/// Toggles content protection for the main window.
fn apply_ignore_self(app: &tauri::AppHandle, ignore_self: bool)
{
    if let Some(window) = app.get_webview_window("main")
    {
        if window.set_content_protected(ignore_self).is_err()
        {
            eprintln!("config: failed to update screenshot protection");
        }
    }
}


/// Builds `count` unique random strings with the app's random string helper.
fn unique_random_strings(count: usize) -> Vec<String>
{
    let mut seen = HashSet::with_capacity(count);
    let mut values = Vec::with_capacity(count);

    while values.len() < count
    {
        let value = file_helper::random_string();
        if seen.insert(value.clone())
        {
            values.push(value);
        }
    }

    values
}
