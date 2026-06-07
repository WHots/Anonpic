//! Application configuration: the settings model and saving it to the working
//! directory's `config/app.cfg`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

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
    /// Prevent Anonpic's own window from appearing in screenshots.
    #[serde(default)]
    pub ignore_self: bool,
    /// Replace stripped image data with user-configured values after cleaning.
    #[serde(default)]
    pub fill_custom_data: bool,
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

/// Serde default for `auto_save`, preserving the app's original always-save
/// behavior for older config files that lack the field.
fn default_true() -> bool
{
    true
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

/// Writes `config` to the config file, ensuring its directory exists first.
fn persist_config(config: &Config) -> bool
{
    let dir = match config_dir()
    {
        Some(dir) => dir,
        None => return false,
    };

    let dir = dir.to_string_lossy().into_owned();
    if !file_helper::create_directory(&dir)
    {
        return false;
    }

    let json = match serde_json::to_string_pretty(config)
    {
        Ok(json) => json,
        Err(_) => return false,
    };

    let path = Path::new(&dir).join(CONFIG_FILE);
    std::fs::write(path, json).is_ok()
}

/// Toggles content protection for the main window.
fn apply_ignore_self(app: &tauri::AppHandle, ignore_self: bool)
{
    if let Some(window) = app.get_webview_window("main")
    {
        let _ = window.set_content_protected(ignore_self);
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

/// Returns `<working_dir>/config`, or `None` if the working directory cannot be
/// determined.
fn config_dir() -> Option<PathBuf>
{
    let mut dir = std::env::current_dir().ok()?;
    dir.push(CONFIG_DIR);
    Some(dir)
}
