//! Free-roam region screenshot: a dimmed full-screen overlay that lets the user
//! drag-select an area with the left mouse button and saves it on release.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC,
    DeleteObject, EndPaint, FillRect, FrameRect, GetStockObject, GetTextExtentPoint32W,
    InvalidateRect, SelectObject, SetBkMode, SetTextColor, TextOutW, DEFAULT_GUI_FONT, HBITMAP,
    HDC, HGDIOBJ, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, LoadCursorW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, SetLayeredWindowAttributes, ShowWindow, TranslateMessage, IDC_CROSS,
    LWA_ALPHA, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SW_SHOW, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_RBUTTONDOWN, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::core::base::saves::user_saves;
use crate::core::helpers::graphics::screen_capture::Screenshot;

// Opacity of the dimming overlay (0 = transparent, 255 = opaque).
const OVERLAY_ALPHA: u8 = 110;

/// Live state of the drag selection, shared between the driver and the window
/// procedure (both run on the same thread).
#[derive(Clone, Copy)]
struct Selection
{
    dragging: bool,
    committed: bool,
    cancelled: bool,
    start: POINT,
    current: POINT,
}

impl Default for Selection
{
    fn default() -> Self
    {
        Self
        {
            dragging: false,
            committed: false,
            cancelled: false,
            start: POINT { x: 0, y: 0 },
            current: POINT { x: 0, y: 0 },
        }
    }
}

thread_local!
{
    static SELECTION: RefCell<Selection> = RefCell::new(Selection::default());
    static BACK_BUFFER: RefCell<Option<BackBuffer>> = RefCell::new(None);
}

/// Snapshots the virtual desktop, shows the selection overlay, and saves the
/// chosen region as a cleaned PNG. Returns the saved path, or `None` if the user
/// cancelled or any step failed.
pub fn capture_and_save() -> Option<PathBuf>
{
    let (origin_x, origin_y, width, height) = virtual_screen();

    if width <= 0 || height <= 0
    {
        return None;
    }


    let snapshot = Screenshot::capture_region(origin_x, origin_y, width, height)?;

    SELECTION.with(|selection| *selection.borrow_mut() = Selection::default());

    let hwnd = create_overlay(origin_x, origin_y, width, height)?;

    pump_messages();

    unsafe { DestroyWindow(hwnd) };

    let selection = SELECTION.with(|selection| *selection.borrow());

    if selection.cancelled || !selection.committed
    {
        return None;
    }

    let region = normalized_rect(selection.start, selection.current);
    let region_width = region.right - region.left;

    let region_height = region.bottom - region.top;

    if region_width <= 0 || region_height <= 0
    {
        return None;
    }

    let cropped = snapshot.crop(region.left, region.top, region_width, region_height)?;
    user_saves::save_screenshot(&cropped)
}


/// Runs a region capture on its own thread so the caller (a key hook or a UI
/// command) never blocks on the overlay's message loop.
pub fn spawn_capture()
{
    std::thread::spawn(||
    {
        let _ = capture_and_save();
    });
}


/// Manually starts a free-roam region capture from the UI, for when the
/// Print Screen hotkey is unavailable.
#[tauri::command]
pub fn start_free_roam_capture()
{
    spawn_capture();
}


/// Returns the virtual desktop as `(origin_x, origin_y, width, height)`.
fn virtual_screen() -> (i32, i32, i32, i32)
{
    let origin_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let origin_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    
    (origin_x, origin_y, width, height)
}

/// Creates and shows the topmost layered overlay covering the given rectangle.
fn create_overlay(x: i32, y: i32, width: i32, height: i32) -> Option<HWND>
{
    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
    let class_name: Vec<u16> = "AnonpicRegionOverlay\0".encode_utf16().collect();
    let cursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_CROSS) };

    let wnd_class = WNDCLASSW
    {
        style: 0,
        lpfnWndProc: Some(overlay_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: ptr::null_mut(),
        hCursor: cursor,
        hbrBackground: ptr::null_mut(),
        lpszMenuName: ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };

    // Registering again after a previous capture fails harmlessly; the class
    // persists for the process, so creation below still succeeds.
    unsafe { RegisterClassW(&wnd_class) };

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            ptr::null(),
            WS_POPUP,
            x,
            y,
            width,
            height,
            ptr::null_mut(),
            ptr::null_mut(),
            hinstance,
            ptr::null(),
        )
    };
    if hwnd.is_null()
    {
        return None;
    }

    unsafe { SetLayeredWindowAttributes(hwnd, 0, OVERLAY_ALPHA, LWA_ALPHA) };
    unsafe { ShowWindow(hwnd, SW_SHOW) };
    unsafe { SetForegroundWindow(hwnd) };
    Some(hwnd)
}

/// Pumps the thread's message queue until the overlay posts `WM_QUIT`.
fn pump_messages()
{
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0
    {
        unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
    }
}

/// Repaints the overlay through an off-screen buffer and blits the result in one
/// pass, so a fast drag stays flicker-free instead of redrawing onto the layered
/// window live.
fn paint_overlay(hwnd: HWND)
{
    let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.is_null()
    {
        return;
    }

    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(hwnd, &mut client) };
    let width = client.right - client.left;
    let height = client.bottom - client.top;

    BACK_BUFFER.with(|buffer|
    {
        let mut slot = buffer.borrow_mut();
        match BackBuffer::memory_dc(&mut slot, hdc, width, height)
        {
            Some(memory_dc) =>
            {
                paint_scene(memory_dc, &client);
                unsafe { BitBlt(hdc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY) };
            }
            // No buffer available: draw straight to the window so a paint still happens.
            None => paint_scene(hdc, &client),
        }
    });

    unsafe { EndPaint(hwnd, &ps) };
}

/// Draws the dimmed backdrop, the selection outline, and the live size label onto
/// `hdc`, which is normally the back buffer.
fn paint_scene(hdc: HDC, client: &RECT)
{
    let background = unsafe { CreateSolidBrush(rgb(20, 20, 20)) };
    if !background.is_null()
    {
        unsafe { FillRect(hdc, client, background) };
        unsafe { DeleteObject(background) };
    }

    let selection = SELECTION.with(|selection| *selection.borrow());
    if selection.dragging || selection.committed
    {
        let region = normalized_rect(selection.start, selection.current);
        if region.right > region.left && region.bottom > region.top
        {
            let border = unsafe { CreateSolidBrush(rgb(255, 255, 255)) };
            if !border.is_null()
            {
                unsafe { FrameRect(hdc, &region, border) };
                unsafe { DeleteObject(border) };
            }
        }

        if selection.dragging
        {
            draw_size_label(hdc, client, &region, selection.current);
        }
    }
}

/// Off-screen surface reused across the drag's repaints; rebuilt only when the
/// client size changes.
struct BackBuffer
{
    dc: HDC,
    bitmap: HBITMAP,
    previous_bitmap: HGDIOBJ,
    width: i32,
    height: i32,
}

impl BackBuffer
{
    /// Returns a memory DC sized to `width`×`height`, reusing the cached buffer
    /// when the size is unchanged and rebuilding it otherwise. Returns `None`
    /// for a non-positive size or if allocation fails.
    fn memory_dc(slot: &mut Option<BackBuffer>, window_dc: HDC, width: i32, height: i32) -> Option<HDC>
    {
        if width <= 0 || height <= 0
        {
            return None;
        }

        if let Some(existing) = slot
        {
            if existing.width == width && existing.height == height
            {
                return Some(existing.dc);
            }
        }

        // Drop any stale buffer before building one at the new size.
        *slot = None;

        let dc = unsafe { CreateCompatibleDC(window_dc) };
        if dc.is_null()
        {
            return None;
        }

        let bitmap = unsafe { CreateCompatibleBitmap(window_dc, width, height) };
        if bitmap.is_null()
        {
            unsafe { DeleteDC(dc) };
            return None;
        }

        let previous_bitmap = unsafe { SelectObject(dc, bitmap) };
        *slot = Some(BackBuffer { dc, bitmap, previous_bitmap, width, height });
        Some(dc)
    }
}

impl Drop for BackBuffer
{
    fn drop(&mut self)
    {
        // SAFETY: restore the DC's original bitmap, then free our bitmap and DC.
        unsafe { SelectObject(self.dc, self.previous_bitmap) };
        unsafe { DeleteObject(self.bitmap) };
        unsafe { DeleteDC(self.dc) };
    }
}


/// Draws a small "width × height" label tracking the cursor while the user
/// drags, on a dark backing box so it stays legible over the screenshot. The
/// label flips to the other side of the cursor when it would clip a client edge.
fn draw_size_label(hdc: HDC, client: &RECT, region: &RECT, cursor: POINT)
{
    let width = region.right - region.left;
    let height = region.bottom - region.top;

    let text: Vec<u16> = format!("{} × {}", width, height).encode_utf16().collect();

    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    let previous_font = unsafe { SelectObject(hdc, font) };

    let mut size = SIZE { cx: 0, cy: 0 };
    unsafe { GetTextExtentPoint32W(hdc, text.as_ptr(), text.len() as i32, &mut size) };

    const PADDING: i32 = 4;
    const OFFSET: i32 = 14;
    let box_width = size.cx + PADDING * 2;
    let box_height = size.cy + PADDING * 2;

    let mut left = cursor.x + OFFSET;
    if left + box_width > client.right
    {
        left = cursor.x - OFFSET - box_width;
    }
    let mut top = cursor.y + OFFSET;
    if top + box_height > client.bottom
    {
        top = cursor.y - OFFSET - box_height;
    }
    left = left.max(0);
    top = top.max(0);

    let box_rect = RECT { left, top, right: left + box_width, bottom: top + box_height };

    let backing = unsafe { CreateSolidBrush(rgb(20, 20, 20)) };
    if !backing.is_null()
    {
        unsafe { FillRect(hdc, &box_rect, backing) };
        unsafe { DeleteObject(backing) };
    }

    unsafe { SetBkMode(hdc, TRANSPARENT as i32) };
    unsafe { SetTextColor(hdc, rgb(255, 255, 255)) };
    unsafe { TextOutW(hdc, left + PADDING, top + PADDING, text.as_ptr(), text.len() as i32) };

    unsafe { SelectObject(hdc, previous_font) };
}

/// Window procedure driving the drag selection.
unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT
{
    match msg
    {
        WM_LBUTTONDOWN =>
        {
            let point = lparam_to_point(lparam);
            SELECTION.with(|selection| {
                let mut selection = selection.borrow_mut();
                selection.dragging = true;
                selection.start = point;
                selection.current = point;
            });
            SetCapture(hwnd);
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_MOUSEMOVE =>
        {
            let point = lparam_to_point(lparam);
            let dragging = SELECTION.with(|selection| {
                let mut selection = selection.borrow_mut();
                if selection.dragging
                {
                    selection.current = point;
                }
                selection.dragging
            });
            if dragging
            {
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP =>
        {
            let point = lparam_to_point(lparam);
            SELECTION.with(|selection| {
                let mut selection = selection.borrow_mut();
                if selection.dragging
                {
                    selection.dragging = false;
                    selection.current = point;
                    selection.committed = true;
                }
            });
            ReleaseCapture();
            PostQuitMessage(0);
            0
        }
        WM_RBUTTONDOWN =>
        {
            SELECTION.with(|selection| selection.borrow_mut().cancelled = true);
            PostQuitMessage(0);
            0
        }
        WM_KEYDOWN =>
        {
            if wparam as u16 == VK_ESCAPE
            {
                SELECTION.with(|selection| selection.borrow_mut().cancelled = true);
                PostQuitMessage(0);
            }
            0
        }
        WM_PAINT =>
        {
            paint_overlay(hwnd);
            0
        }
        WM_DESTROY =>
        {
            BACK_BUFFER.with(|buffer| *buffer.borrow_mut() = None);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Extracts the signed `(x, y)` client coordinates packed into a mouse `lParam`.
fn lparam_to_point(lparam: LPARAM) -> POINT
{
    let x = (lparam & 0xFFFF) as i16 as i32;
    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
    POINT { x, y }
}

/// Builds a normalized rectangle (left <= right, top <= bottom) from two points.
fn normalized_rect(a: POINT, b: POINT) -> RECT
{
    RECT
    {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    }
}

/// Packs an RGB triple into a Win32 `COLORREF`.
fn rgb(r: u8, g: u8, b: u8) -> COLORREF
{
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}
