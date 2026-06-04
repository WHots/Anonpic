//! Native Windows toast notifications surfaced through `notify-rust`. The Tauri
//! `AppHandle` is captured once at startup so the bundled `logo.png` resource can
//! be resolved and so notifications can be raised from the capture threads, which
//! have no handle of their own.

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};

/// AppUserModelID that tags every toast. Matches the Tauri bundle identifier so
/// installed builds share the identity registered by the installer. Without it,
/// `notify-rust` falls back to PowerShell's identity (name and icon).
const APP_ID: &str = "com.anonpic.app";

/// Friendly name Windows shows as the toast's source.
const DISPLAY_NAME: &str = "Anonpic";

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Stores the Tauri `AppHandle` for later notifications and registers the app's
/// toast identity. Called once from the app's setup hook; further calls are
/// ignored.
pub fn init(app: AppHandle)
{
    let _ = APP_HANDLE.set(app);
    register_app_id();
}

/// Raises a toast notification, badged with the app logo, telling the user a
/// screenshot was saved at `path`.
pub fn notify_screenshot_saved(path: &Path)
{
    let mut notification = notify_rust::Notification::new();
    notification
        .app_id(APP_ID)
        .summary("Screenshot saved")
        .body(&path.to_string_lossy());

    if let Some(logo) = logo_path()
    {
        notification.image_path(&logo);
    }

    let _ = notification.show();
}


/// Alerts the user that their screenshot was copied to the clipboard. When
/// `saved_path` is `Some`, the capture was also kept on disk and the path is
/// shown; when `None`, only the clipboard copy was made and no path is implied.
pub fn notify_screenshot_clipboardsaved(saved_path: Option<&Path>)
{
    let mut notification = notify_rust::Notification::new();
    notification.app_id(APP_ID).summary("Screenshot copied to clipboard");

    match saved_path
    {
        Some(path) => notification.body(&path.to_string_lossy()),
        None => notification.body("The capture is on your clipboard."),
    };

    if let Some(logo) = logo_path()
    {
        notification.image_path(&logo);
    }

    let _ = notification.show();
}


/// Registers the AppUserModelID under `HKCU` so Windows attributes toasts to
/// this app with its own name and icon instead of the PowerShell host that
/// `notify-rust` targets by default. Idempotent; safe to run on every startup.
fn register_app_id()
{
    let subkey = wide(&format!("Software\\Classes\\AppUserModelId\\{APP_ID}"));

    // SAFETY: every pointer references a local that outlives the call, and an
    // opened key is always closed before returning.
    unsafe
    {
        let mut key: HKEY = std::ptr::null_mut();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != 0
        {
            return;
        }

        set_string(key, "DisplayName", DISPLAY_NAME);
        if let Some(icon) = icon_path()
        {
            set_string(key, "IconUri", &icon);
        }

        RegCloseKey(key);
    }
}

/// Writes a single `REG_SZ` value into an already-opened registry key.
unsafe fn set_string(key: HKEY, name: &str, value: &str)
{
    let name = wide(name);
    let data = wide(value);
    RegSetValueExW(
        key,
        name.as_ptr(),
        0,
        REG_SZ,
        data.as_ptr() as *const u8,
        (data.len() * std::mem::size_of::<u16>()) as u32,
    );
}

/// Encodes a string as a NUL-terminated UTF-16 buffer for the Win32 `*W` APIs.
fn wide(value: &str) -> Vec<u16>
{
    OsStr::new(value).encode_wide().chain(once(0)).collect()
}

/// Resolves the app logo: the bundled `logo.png` resource for installed builds,
/// falling back to `ui/logo.png` in the source tree during development. Returns
/// `None` if neither exists.
fn logo_path() -> Option<String>
{
    if let Some(handle) = APP_HANDLE.get()
    {
        if let Ok(resource) = handle.path().resolve("logo.png", BaseDirectory::Resource)
        {
            if resource.exists()
            {
                return Some(resource.to_string_lossy().into_owned());
            }
        }
    }

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.join("ui").join("logo.png");
    if source.exists()
    {
        return Some(source.to_string_lossy().into_owned());
    }

    None
}

/// Resolves the multi-resolution `icon.ico` used as the toast's app icon: the
/// bundled resource for installed builds, falling back to `src-tauri/icons` in
/// the source tree during development. Returns `None` if neither exists.
fn icon_path() -> Option<String>
{
    if let Some(handle) = APP_HANDLE.get()
    {
        if let Ok(resource) = handle.path().resolve("icon.ico", BaseDirectory::Resource)
        {
            if resource.exists()
            {
                return Some(resource.to_string_lossy().into_owned());
            }
        }
    }

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("icons").join("icon.ico");
    if source.exists()
    {
        return Some(source.to_string_lossy().into_owned());
    }

    None
}
