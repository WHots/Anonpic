//! Free-roam region screenshot: a dimmed full-screen overlay that lets the user
//! drag-select an area with the left mouse button and saves it on release.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, Ellipse, EndPaint, FillRect, FrameRect, GetStockObject,
    GetTextExtentPoint32W, InvalidateRect, SelectObject, SetBkMode, SetTextColor, TextOutW,
    DEFAULT_GUI_FONT, HBITMAP, HDC, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY,
    TRANSPARENT,
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

use crate::core::base::configs::config_master;
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
    /// An idle selection with no drag in progress at the origin.
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

        *slot = None;

        // SAFETY: `window_dc` is live, created handles are checked, and ownership
        // of the selected bitmap and DC moves into the returned `BackBuffer`.
        unsafe
        {
            let dc = CreateCompatibleDC(window_dc);
            if dc.is_null()
            {
                eprintln!("overlay: failed to create back-buffer DC");
                return None;
            }

            let bitmap = CreateCompatibleBitmap(window_dc, width, height);
            if bitmap.is_null()
            {
                eprintln!("overlay: failed to create back-buffer bitmap");
                DeleteDC(dc);
                return None;
            }

            let previous_bitmap = SelectObject(dc, bitmap);
            if previous_bitmap.is_null()
            {
                eprintln!("overlay: failed to select the back-buffer bitmap");
                DeleteObject(bitmap);
                DeleteDC(dc);
                return None;
            }
            *slot = Some(BackBuffer { dc, bitmap, previous_bitmap, width, height });
            Some(dc)
        }
    }
}

impl Drop for BackBuffer
{
    /// Restores the DC's original bitmap and frees the buffer's GDI objects.
    fn drop(&mut self)
    {
        // SAFETY: this buffer owns `dc` and `bitmap`; the original object is
        // restored before both owned handles are released exactly once.
        unsafe
        {
            SelectObject(self.dc, self.previous_bitmap);
            if DeleteObject(self.bitmap) == 0
            {
                eprintln!("overlay: failed to release the back-buffer bitmap");
            }
            if DeleteDC(self.dc) == 0
            {
                eprintln!("overlay: failed to release the back-buffer DC");
            }
        }
    }
}

thread_local!
{
    static SELECTION: RefCell<Selection> = RefCell::new(Selection::default());
    static BACK_BUFFER: RefCell<Option<BackBuffer>> = const { RefCell::new(None) };
    static CIRCULAR: Cell<bool> = const { Cell::new(false) };
    static FROZEN_BITMAP: Cell<HBITMAP> = const { Cell::new(ptr::null_mut()) };
}


/// Shows the selection overlay and saves the chosen region as a cleaned image.
/// When configured, the overlay displays and crops the same frozen desktop
/// frame; otherwise the selected region is captured after the overlay closes.
/// Returns the saved path, or `None` if the user cancelled or a step failed.
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

    CIRCULAR.with(|flag| flag.set(circular));
    FROZEN_BITMAP.with(|bitmap| bitmap.set(snapshot.as_ref().map(Screenshot::bitmap).unwrap_or(ptr::null_mut())));

    SELECTION.with(|selection| *selection.borrow_mut() = Selection::default());

    let hwnd = match create_overlay(origin_x, origin_y, width, height, freeze_screen)
    {
        Some(hwnd) => hwnd,
        None =>
        {
            FROZEN_BITMAP.with(|bitmap| bitmap.set(ptr::null_mut()));
            return None;
        }
    };

    pump_messages();

    // SAFETY: `hwnd` is the overlay window created above on this thread.
    unsafe { DestroyWindow(hwnd) };
    FROZEN_BITMAP.with(|bitmap| bitmap.set(ptr::null_mut()));

    if snapshot.is_none()
    {
        // SAFETY: `DwmFlush` takes no pointers and only waits for pending desktop
        // composition work so the destroyed overlay is absent from the capture.
        if unsafe { DwmFlush() } != 0
        {
            eprintln!("overlay: failed to flush desktop composition");
        }
    }

    let selection = SELECTION.with(|selection| *selection.borrow());

    if selection.cancelled || !selection.committed
    {
        return None;
    }

    let bounds = RECT { left: 0, top: 0, right: width, bottom: height };
    let region = if circular { circle_rect(selection.start, selection.current, &bounds) } else { normalized_rect(selection.start, selection.current) };
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


/// Creates and shows the topmost layered overlay covering the given rectangle.
/// Uses full opacity when `freeze_screen` is set so the frozen bitmap fully
/// replaces the changing desktop beneath it.
/// Registering the window class again after a previous capture fails
/// harmlessly; the class persists for the process, so creation still succeeds.
/// Returns `None` when the window cannot be created.
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


/// Pumps the thread's message queue until the overlay posts `WM_QUIT`.
fn pump_messages()
{
    // SAFETY: `msg` is initialized for Win32 and remains live while each
    // retrieved message is translated and dispatched on this thread.
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


/// Repaints the overlay through an off-screen buffer and blits the result in one
/// pass, so a fast drag stays flicker-free instead of redrawing onto the layered
/// window live. When no buffer is available, draws straight to the window so a
/// paint still happens.
fn paint_overlay(hwnd: HWND)
{
    // SAFETY: `hwnd` comes from this module's window procedure, paint and client
    // structures stay live, and every successful `BeginPaint` is ended once.
    unsafe
    {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        if hdc.is_null()
        {
            eprintln!("overlay: failed to begin painting");
            return;
        }

        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetClientRect(hwnd, &mut client) == 0
        {
            eprintln!("overlay: failed to read the client rectangle");
        }
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
                    if BitBlt(hdc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY) == 0
                    {
                        eprintln!("overlay: failed to copy the back buffer");
                    }
                }
                None => paint_scene(hdc, &client),
            }
        });

        if EndPaint(hwnd, &ps) == 0
        {
            eprintln!("overlay: failed to finish painting");
        }
    }
}


/// Draws either the frozen desktop or dimmed live backdrop, then the selection
/// outline and live size label, onto `hdc`.
fn paint_scene(hdc: HDC, client: &RECT)
{
    let frozen_bitmap = FROZEN_BITMAP.with(|bitmap| bitmap.get());
    if frozen_bitmap.is_null()
    {
        // SAFETY: `hdc` and `client` are live for painting, and the created brush
        // is checked and released before leaving this region.
        unsafe
        {
            let background = CreateSolidBrush(rgb(20, 20, 20));
            if background.is_null()
            {
                eprintln!("overlay: failed to create the background brush");
            }
            else
            {
                if FillRect(hdc, client, background) == 0
                {
                    eprintln!("overlay: failed to paint the background");
                }
                if DeleteObject(background) == 0
                {
                    eprintln!("overlay: failed to release the background brush");
                }
            }
        }
    }
    else
    {
        // SAFETY: `frozen_bitmap` is owned by the live snapshot for the entire
        // message loop; selected objects are restored and the temporary DC freed.
        unsafe
        {
            let source_dc = CreateCompatibleDC(hdc);
            if source_dc.is_null()
            {
                eprintln!("overlay: failed to create the frozen-frame DC");
            }
            else
            {
                let previous = SelectObject(source_dc, frozen_bitmap);
                if previous.is_null()
                {
                    eprintln!("overlay: failed to select the frozen frame");
                }
                else
                {
                    let width = client.right - client.left;
                    let height = client.bottom - client.top;
                    if BitBlt(hdc, 0, 0, width, height, source_dc, 0, 0, SRCCOPY) == 0
                    {
                        eprintln!("overlay: failed to draw the frozen frame");
                    }
                    SelectObject(source_dc, previous);
                }
                if DeleteDC(source_dc) == 0
                {
                    eprintln!("overlay: failed to release the frozen-frame DC");
                }
            }
        }
    }

    let selection = SELECTION.with(|selection| *selection.borrow());
    if selection.dragging || selection.committed
    {
        let circular = CIRCULAR.with(|flag| flag.get());
        let region = if circular { circle_rect(selection.start, selection.current, client) } else { normalized_rect(selection.start, selection.current) };
        if region.right > region.left && region.bottom > region.top
        {
            if circular
            {
                draw_circular_selection(hdc, &region);
            }
            else
            {
                // SAFETY: `hdc` and `region` are live, and the temporary brush
                // is checked and released after the frame is drawn.
                unsafe
                {
                    let border = CreateSolidBrush(rgb(255, 255, 255));
                    if border.is_null()
                    {
                        eprintln!("overlay: failed to create the selection brush");
                    }
                    else
                    {
                        if FrameRect(hdc, &region, border) == 0
                        {
                            eprintln!("overlay: failed to draw the selection frame");
                        }
                        if DeleteObject(border) == 0
                        {
                            eprintln!("overlay: failed to release the selection brush");
                        }
                    }
                }
            }
        }

        if selection.dragging
        {
            draw_size_label(hdc, client, &region, selection.current);
        }
    }
}


/// Draws the selection region as a hollow white circle outline onto `hdc`,
/// used when circular selection is enabled in the settings. `region` is the
/// circle's bounding square, so the ellipse call renders a true circle.
fn draw_circular_selection(hdc: HDC, region: &RECT)
{
    // SAFETY: `hdc` and `region` are live, created objects are checked, selected
    // objects are restored, and the owned pen is released exactly once.
    unsafe
    {
        let pen = CreatePen(PS_SOLID, 1, rgb(255, 255, 255));
        if pen.is_null()
        {
            eprintln!("overlay: failed to create selection pen");
            return;
        }

        let hollow = GetStockObject(NULL_BRUSH);
        if hollow.is_null()
        {
            eprintln!("overlay: failed to load the hollow brush");
            DeleteObject(pen);
            return;
        }

        let previous_pen = SelectObject(hdc, pen);
        let previous_brush = SelectObject(hdc, hollow);
        if Ellipse(hdc, region.left, region.top, region.right, region.bottom) == 0
        {
            eprintln!("overlay: failed to draw the circular selection");
        }

        SelectObject(hdc, previous_pen);
        SelectObject(hdc, previous_brush);
        if DeleteObject(pen) == 0
        {
            eprintln!("overlay: failed to release the selection pen");
        }
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

    // SAFETY: `hdc`, `client`, and `region` are live, `text` outlives each call,
    // selected objects are restored, and the temporary brush is released.
    unsafe
    {
        let font = GetStockObject(DEFAULT_GUI_FONT);
        if font.is_null()
        {
            eprintln!("overlay: failed to load the label font");
            return;
        }
        let previous_font = SelectObject(hdc, font);

        let mut size = SIZE { cx: 0, cy: 0 };
        if GetTextExtentPoint32W(hdc, text.as_ptr(), text.len() as i32, &mut size) == 0
        {
            eprintln!("overlay: failed to measure the size label");
        }

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

        let backing = CreateSolidBrush(rgb(20, 20, 20));
        if backing.is_null()
        {
            eprintln!("overlay: failed to create the label background brush");
        }
        else
        {
            if FillRect(hdc, &box_rect, backing) == 0
            {
                eprintln!("overlay: failed to paint the label background");
            }
            if DeleteObject(backing) == 0
            {
                eprintln!("overlay: failed to release the label background brush");
            }
        }

        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, rgb(255, 255, 255));
        if TextOutW(hdc, left + PADDING, top + PADDING, text.as_ptr(), text.len() as i32) == 0
        {
            eprintln!("overlay: failed to draw the size label");
        }

        SelectObject(hdc, previous_font);
    }
}


/// Window procedure for the selection overlay: tracks the drag in `SELECTION`,
/// repaints on movement, and posts `WM_QUIT` on commit (left-button release) or
/// cancel (right button or Escape).
///
/// SAFETY: called by the system on the overlay's thread with a valid `hwnd` and
/// message parameters for that window.
unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT
{
    match msg
    {
        WM_LBUTTONDOWN =>
        {
            let point = lparam_to_point(lparam);
            SELECTION.with(|selection|
            {
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
            let dragging = SELECTION.with(|selection|
            {
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
            SELECTION.with(|selection|
            {
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


/// Builds the bounding square of a true circle centered at `center` whose
/// radius is the euclidean distance to `edge`, clamped so the square never
/// leaves `bounds`.
fn circle_rect(center: POINT, edge: POINT, bounds: &RECT) -> RECT
{
    let dx = (edge.x - center.x) as f64;
    let dy = (edge.y - center.y) as f64;
    let distance = (dx * dx + dy * dy).sqrt();

    let max_radius = (center.x - bounds.left).min(bounds.right - center.x).min(center.y - bounds.top).min(bounds.bottom - center.y).max(0);
    let radius = distance.min(max_radius as f64) as i32;

    RECT
    {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius,
        bottom: center.y + radius,
    }
}


/// Packs an RGB triple into a Win32 `COLORREF`.
fn rgb(r: u8, g: u8, b: u8) -> COLORREF
{
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}
