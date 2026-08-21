//! Window lifecycle and input state for the region-selection overlay.

use std::cell::RefCell;
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::HBITMAP;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LoadCursorW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetLayeredWindowAttributes, ShowWindow, TranslateMessage, IDC_CROSS, LWA_ALPHA, MSG,
    SW_SHOW, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::drawing;
use super::math_utils::lparam_to_point;

/// Opacity of the dimmed overlay when the desktop remains live.
const OVERLAY_ALPHA: u8 = 110;

/// Current drag-selection state for the overlay thread.
#[derive(Clone, Copy)]
pub(super) struct Selection
{
    pub(super) dragging: bool,
    pub(super) committed: bool,
    pub(super) start: POINT,
    pub(super) current: POINT,
    cancelled: bool,
}

impl Default for Selection
{
    /// Returns an idle selection at the client origin.
    fn default() -> Self
    {
        Self
        {
            dragging: false,
            committed: false,
            start: POINT { x: 0, y: 0 },
            current: POINT { x: 0, y: 0 },
            cancelled: false,
        }
    }
}


/// Thread-local values read by the Win32 window procedure while the overlay runs.
#[derive(Clone, Copy)]
struct OverlayState
{
    selection: Selection,
    circular: bool,
    frozen_bitmap: HBITMAP,
}

impl Default for OverlayState
{
    /// Returns overlay state with no active selection or borrowed bitmap.
    fn default() -> Self
    {
        Self
        {
            selection: Selection::default(),
            circular: false,
            frozen_bitmap: ptr::null_mut(),
        }
    }
}


thread_local! {
    static STATE: RefCell<OverlayState> = RefCell::new(OverlayState::default());
}


/// Runs the overlay and returns its committed selection, or `None` on failure or cancellation.
pub(super) fn select_region(x: i32, y: i32, width: i32, height: i32, freeze_screen: bool, circular: bool, frozen_bitmap: HBITMAP) -> Option<Selection>
{
    STATE.with(|state| {
        *state.borrow_mut() = OverlayState
        {
            selection: Selection::default(),
            circular,
            frozen_bitmap,
        };
    });

    let hwnd = match create_overlay(x, y, width, height, freeze_screen)
    {
        Some(hwnd) => hwnd,
        None =>
        {
            reset_state();
            return None;
        }
    };

    pump_messages();

    // SAFETY: `hwnd` is the overlay window created above on this thread.
    unsafe { DestroyWindow(hwnd) };

    if !freeze_screen
    {
        // SAFETY: `DwmFlush` takes no pointers and waits only for pending desktop
        // composition work so the destroyed overlay is absent from the capture.
        if unsafe { DwmFlush() } != 0
        {
            eprintln!("overlay: failed to flush desktop composition");
        }
    }

    let selection = STATE.with(|state| state.borrow().selection);
    reset_state();

    if selection.cancelled || !selection.committed
    {
        None
    }
    else
    {
        Some(selection)
    }
}


/// Clears borrowed handles and input state after an overlay completes.
fn reset_state()
{
    STATE.with(|state| *state.borrow_mut() = OverlayState::default());
}


/// Creates and shows the topmost layered selection window.
fn create_overlay(x: i32, y: i32, width: i32, height: i32, freeze_screen: bool) -> Option<HWND>
{
    // SAFETY: all class and title pointers remain live through registration and
    // creation; the returned window handle is checked before subsequent calls.
    unsafe
    {
        let hinstance = GetModuleHandleW(ptr::null());
        if hinstance.is_null()
        {
            eprintln!("overlay: failed to resolve the executable module");
            return None;
        }

        let class_name: Vec<u16> = "AnonpicRegionOverlay\0".encode_utf16().collect();
        let cursor = LoadCursorW(ptr::null_mut(), IDC_CROSS);
        if cursor.is_null()
        {
            eprintln!("overlay: failed to load the selection cursor");
            return None;
        }

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

        RegisterClassW(&wnd_class);

        let hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class_name.as_ptr(), ptr::null(), WS_POPUP, x, y, width, height, ptr::null_mut(), ptr::null_mut(), hinstance, ptr::null());
        if hwnd.is_null()
        {
            eprintln!("overlay: failed to create overlay window");
            return None;
        }

        let alpha = if freeze_screen { u8::MAX } else { OVERLAY_ALPHA };
        if SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) == 0
        {
            eprintln!("overlay: failed to apply overlay transparency");
        }
        ShowWindow(hwnd, SW_SHOW);
        if SetForegroundWindow(hwnd) == 0
        {
            eprintln!("overlay: failed to focus the overlay window");
        }
        Some(hwnd)
    }
}


/// Pumps the current thread's message queue until the overlay posts `WM_QUIT`.
fn pump_messages()
{
    // SAFETY: `msg` remains live while each retrieved message is translated and
    // dispatched on the overlay thread.
    unsafe
    {
        let mut msg: MSG = std::mem::zeroed();
        loop
        {
            let status = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
            if status == 0
            {
                break;
            }
            if status < 0
            {
                eprintln!("overlay: failed to read the message queue");
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}


/// Handles drag input, repaint requests, commit, and cancellation messages.
///
/// SAFETY: Windows calls this procedure on the overlay thread with parameters
/// belonging to `hwnd`.
unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT
{
    match msg
    {
        WM_LBUTTONDOWN =>
        {
            let point = lparam_to_point(lparam);
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.selection.dragging = true;
                state.selection.start = point;
                state.selection.current = point;
            });
            SetCapture(hwnd);
            drawing::request_repaint(hwnd);
            0
        }
        WM_MOUSEMOVE =>
        {
            let point = lparam_to_point(lparam);
            let dragging = STATE.with(|state| {
                let mut state = state.borrow_mut();
                if state.selection.dragging
                {
                    state.selection.current = point;
                }
                state.selection.dragging
            });
            if dragging
            {
                drawing::request_repaint(hwnd);
            }
            0
        }
        WM_LBUTTONUP =>
        {
            let point = lparam_to_point(lparam);
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                if state.selection.dragging
                {
                    state.selection.dragging = false;
                    state.selection.current = point;
                    state.selection.committed = true;
                }
            });
            ReleaseCapture();
            PostQuitMessage(0);
            0
        }
        WM_RBUTTONDOWN =>
        {
            STATE.with(|state| state.borrow_mut().selection.cancelled = true);
            PostQuitMessage(0);
            0
        }
        WM_KEYDOWN =>
        {
            if wparam as u16 == VK_ESCAPE
            {
                STATE.with(|state| state.borrow_mut().selection.cancelled = true);
                PostQuitMessage(0);
            }
            0
        }
        WM_PAINT =>
        {
            let state = STATE.with(|state| *state.borrow());
            drawing::paint_overlay(hwnd, &state.selection, state.circular, state.frozen_bitmap);
            0
        }
        WM_DESTROY =>
        {
            drawing::release_back_buffer();
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
