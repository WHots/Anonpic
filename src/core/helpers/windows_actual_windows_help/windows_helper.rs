//! Helper routines for performing actual Window operations.

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN,
};




/// Returns the primary monitor's dimensions in pixels as `(width, height)`.
pub fn screen_dimensions() -> (i32, i32)
{
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    (width, height)
}


/// Returns the dimensions in pixels as `(width, height)` of the window
/// identified by `hwnd`, or `None` if its rectangle cannot be read.
pub fn get_dimensions_from_handle(hwnd: HWND) -> Option<(i32, i32)>
{
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { GetWindowRect(hwnd, &mut rect) };

    if ok == 0
    {
        return None;
    }

    Some((rect.right - rect.left, rect.bottom - rect.top))
}