//! Free-roam region capture orchestration.

use std::path::PathBuf;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};

use super::math_utils::{circle_rect, normalized_rect};
use super::overlay;
use crate::core::base::configs::config_master;
use crate::core::base::saves::user_saves;
use crate::core::helpers::graphics::screen_capture::Screenshot;

/// Shows the selection overlay and saves the chosen region as a cleaned image.
/// Returns the saved path, or `None` when cancelled or when a capture step fails.
pub fn capture_and_save() -> Option<PathBuf>
{
    let (origin_x, origin_y, width, height) = virtual_screen();
    if width <= 0 || height <= 0
    {
        return None;
    }

    let config = config_master::load_config();
    let circular = config.as_ref().map(|config| config.circular_selection).unwrap_or(false);
    let freeze_screen = config.as_ref().map(|config| config.freeze_screen_on_capture).unwrap_or(true);
    let snapshot = if freeze_screen { Some(Screenshot::capture_region(origin_x, origin_y, width, height)?) } else { None };
    let frozen_bitmap = snapshot.as_ref().map(Screenshot::bitmap).unwrap_or(std::ptr::null_mut());
    let selection = overlay::select_region(origin_x, origin_y, width, height, freeze_screen, circular, frozen_bitmap)?;

    let bounds = RECT
    {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    let region = if circular
    {
        circle_rect(selection.start, selection.current, &bounds)
    }
    else
    {
        normalized_rect(selection.start, selection.current)
    };
    let region_width = region.right - region.left;
    let region_height = region.bottom - region.top;
    if region_width <= 0 || region_height <= 0
    {
        return None;
    }

    let captured = match snapshot
    {
        Some(snapshot) => snapshot.crop(region.left, region.top, region_width, region_height)?,
        None => Screenshot::capture_region(origin_x + region.left, origin_y + region.top, region_width, region_height)?,
    };
    user_saves::save_screenshot(&captured, circular)
}


/// Runs a region capture on a worker thread so callers remain responsive.
pub fn spawn_capture()
{
    std::thread::spawn(|| {
        let _ = capture_and_save();
    });
}


/// Starts a free-roam region capture from the Tauri UI.
#[tauri::command]
pub fn start_free_roam_capture()
{
    spawn_capture();
}


/// Returns the virtual desktop as `(origin_x, origin_y, width, height)`.
fn virtual_screen() -> (i32, i32, i32, i32)
{
    // SAFETY: these metric indices require no pointers or caller-owned handles.
    unsafe
    {
        let origin_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let origin_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        (origin_x, origin_y, width, height)
    }
}
