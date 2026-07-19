//! Screen capture routines.

use std::ptr;

use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, BLACKNESS, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
    HBITMAP, PatBlt, ReleaseDC, SelectObject, SRCCOPY,
};

/// A screenshot held in a GDI bitmap. Owns the bitmap and frees it on drop.
pub struct Screenshot
{
    bitmap: HBITMAP,
    width: i32,
    height: i32,
}

impl Screenshot
{
    /// Captures a rectangular region of the screen at virtual-desktop
    /// coordinates `(x, y)` with the given size into a GDI bitmap. Returns
    /// `None` for a non-positive size or if the capture fails.
    pub fn capture_region(x: i32, y: i32, width: i32, height: i32) -> Option<Screenshot>
    {
        if width <= 0 || height <= 0
        {
            return None;
        }

        // SAFETY: every DC and GDI object acquired below is released on each
        // path; the bitmap's ownership moves into the returned Screenshot.
        let screen_dc = unsafe { GetDC(ptr::null_mut()) };
        if screen_dc.is_null()
        {
            return None;
        }

        let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
        if memory_dc.is_null()
        {
            unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };
            return None;
        }

        let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
        if bitmap.is_null()
        {
            unsafe { DeleteDC(memory_dc) };
            unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };
            return None;
        }

        let previous = unsafe { SelectObject(memory_dc, bitmap) };
        let copied = unsafe { BitBlt(memory_dc, 0, 0, width, height, screen_dc, x, y, SRCCOPY) };
        unsafe { SelectObject(memory_dc, previous) };
        unsafe { DeleteDC(memory_dc) };
        unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };

        if copied == 0
        {
            unsafe { DeleteObject(bitmap) };
            return None;
        }

        Some(Screenshot { bitmap, width, height })
    }


    /// Copies a sub-rectangle of this screenshot into a new screenshot. `(x, y)`
    /// is relative to this bitmap's top-left. Returns `None` if the rectangle
    /// lies outside the bitmap or the copy fails.
    pub fn crop(&self, x: i32, y: i32, width: i32, height: i32) -> Option<Screenshot>
    {
        if width <= 0 || height <= 0 || x < 0 || y < 0 || x + width > self.width || y + height > self.height
        {
            return None;
        }

        // SAFETY: every DC and GDI object acquired below is released on each
        // path; the bitmap's ownership moves into the returned Screenshot.
        let screen_dc = unsafe { GetDC(ptr::null_mut()) };
        if screen_dc.is_null()
        {
            return None;
        }

        let source_dc = unsafe { CreateCompatibleDC(screen_dc) };
        let dest_dc = unsafe { CreateCompatibleDC(screen_dc) };
        let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
        unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };

        if source_dc.is_null() || dest_dc.is_null() || bitmap.is_null()
        {
            if !source_dc.is_null()
            {
                unsafe { DeleteDC(source_dc) };
            }
            if !dest_dc.is_null()
            {
                unsafe { DeleteDC(dest_dc) };
            }
            if !bitmap.is_null()
            {
                unsafe { DeleteObject(bitmap) };
            }
            return None;
        }

        let previous_source = unsafe { SelectObject(source_dc, self.bitmap) };
        let previous_dest = unsafe { SelectObject(dest_dc, bitmap) };
        let copied = unsafe { BitBlt(dest_dc, 0, 0, width, height, source_dc, x, y, SRCCOPY) };
        unsafe { SelectObject(source_dc, previous_source) };
        unsafe { SelectObject(dest_dc, previous_dest) };
        unsafe { DeleteDC(source_dc) };
        unsafe { DeleteDC(dest_dc) };

        if copied == 0
        {
            unsafe { DeleteObject(bitmap) };
            return None;
        }

        Some(Screenshot { bitmap, width, height })
    }


    /// The captured bitmap's dimensions in pixels as `(width, height)`.
    pub fn dimensions(&self) -> (i32, i32)
    {
        (self.width, self.height)
    }


    /// The underlying GDI bitmap handle.
    pub fn bitmap(&self) -> HBITMAP
    {
        self.bitmap
    }
}

impl Drop for Screenshot
{
    /// Zeroes the pixel data before freeing the bitmap so the capture does not
    /// linger in freed GDI memory.
    fn drop(&mut self)
    {
        // SAFETY: this screenshot owns `bitmap`; the scratch DC is created and
        // deleted here, and the bitmap is freed exactly once.
        let dc = unsafe { CreateCompatibleDC(ptr::null_mut()) };
        if !dc.is_null()
        {
            let previous = unsafe { SelectObject(dc, self.bitmap) };
            unsafe { PatBlt(dc, 0, 0, self.width, self.height, BLACKNESS) };
            unsafe { SelectObject(dc, previous) };
            unsafe { DeleteDC(dc) };
        }
        unsafe { DeleteObject(self.bitmap) };
    }
}
