//! GDI drawing for the region-selection overlay.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush, DeleteDC, DeleteObject, Ellipse, EndPaint, FillRect, FrameRect, GetStockObject, GetTextExtentPoint32W, InvalidateRect, SelectObject,
    SetBkMode, SetTextColor, TextOutW, DEFAULT_GUI_FONT, HBITMAP, HDC, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

use super::math_utils::{circle_rect, normalized_rect};
use super::overlay::Selection;

/// Off-screen surface reused while the selection is repainted.
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
    /// Returns a reusable memory DC matching the requested dimensions.
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
    /// Restores the original bitmap and releases the owned GDI objects.
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


thread_local! {
    static BACK_BUFFER: RefCell<Option<BackBuffer>> = const { RefCell::new(None) };
}


/// Repaints the complete overlay through the reusable off-screen buffer.
pub(super) fn paint_overlay(hwnd: HWND, selection: &Selection, circular: bool, frozen_bitmap: HBITMAP)
{
    // SAFETY: `hwnd` comes from the overlay window procedure, paint structures
    // remain live, and every successful `BeginPaint` is ended exactly once.
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

        BACK_BUFFER.with(|buffer| {
            let mut slot = buffer.borrow_mut();
            match BackBuffer::memory_dc(&mut slot, hdc, width, height)
            {
                Some(memory_dc) =>
                {
                    paint_scene(memory_dc, &client, selection, circular, frozen_bitmap);
                    if BitBlt(hdc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY) == 0
                    {
                        eprintln!("overlay: failed to copy the back buffer");
                    }
                }
                None => paint_scene(hdc, &client, selection, circular, frozen_bitmap),
            }
        });

        if EndPaint(hwnd, &ps) == 0
        {
            eprintln!("overlay: failed to finish painting");
        }
    }
}


/// Releases the thread's cached drawing surface when the overlay is destroyed.
pub(super) fn release_back_buffer()
{
    BACK_BUFFER.with(|buffer| *buffer.borrow_mut() = None);
}


/// Invalidates the overlay so Windows schedules a repaint.
pub(super) fn request_repaint(hwnd: HWND)
{
    // SAFETY: `hwnd` is the live overlay window and a null rectangle requests
    // invalidation of its complete client area.
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
}


/// Draws the backdrop, selection outline, and optional live size label.
fn paint_scene(hdc: HDC, client: &RECT, selection: &Selection, circular: bool, frozen_bitmap: HBITMAP)
{
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
        draw_frozen_frame(hdc, client, frozen_bitmap);
    }

    if selection.dragging || selection.committed
    {
        let region = if circular
        {
            circle_rect(selection.start, selection.current, client)
        }
        else
        {
            normalized_rect(selection.start, selection.current)
        };
        if region.right > region.left && region.bottom > region.top
        {
            if circular
            {
                draw_circular_selection(hdc, &region);
            }
            else
            {
                draw_rectangular_selection(hdc, &region);
            }
        }

        if selection.dragging
        {
            draw_size_label(hdc, client, &region, selection.current);
        }
    }
}


/// Copies the frozen desktop bitmap into the overlay's drawing surface.
fn draw_frozen_frame(hdc: HDC, client: &RECT, frozen_bitmap: HBITMAP)
{
    // SAFETY: `frozen_bitmap` is owned by the live snapshot during the message
    // loop; selected objects are restored and the temporary DC is released.
    unsafe
    {
        let source_dc = CreateCompatibleDC(hdc);
        if source_dc.is_null()
        {
            eprintln!("overlay: failed to create the frozen-frame DC");
            return;
        }

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


/// Draws the rectangular selection as a white frame.
fn draw_rectangular_selection(hdc: HDC, region: &RECT)
{
    // SAFETY: `hdc` and `region` are live, and the temporary brush is checked
    // and released after the frame is drawn.
    unsafe
    {
        let border = CreateSolidBrush(rgb(255, 255, 255));
        if border.is_null()
        {
            eprintln!("overlay: failed to create the selection brush");
            return;
        }

        if FrameRect(hdc, region, border) == 0
        {
            eprintln!("overlay: failed to draw the selection frame");
        }
        if DeleteObject(border) == 0
        {
            eprintln!("overlay: failed to release the selection brush");
        }
    }
}


/// Draws the selection region as a hollow white circle.
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


/// Draws the live capture dimensions beside the cursor.
fn draw_size_label(hdc: HDC, client: &RECT, region: &RECT, cursor: POINT)
{
    let width = region.right - region.left;
    let height = region.bottom - region.top;
    let text: Vec<u16> = format!("{} × {}", width, height).encode_utf16().collect();

    // SAFETY: all borrowed drawing values remain live through each call,
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

        let box_rect = RECT
        {
            left,
            top,
            right: left + box_width,
            bottom: top + box_height,
        };
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


/// Packs an RGB triple into a Win32 `COLORREF`.
fn rgb(r: u8, g: u8, b: u8) -> COLORREF
{
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}
