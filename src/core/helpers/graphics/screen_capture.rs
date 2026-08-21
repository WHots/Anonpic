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

        // SAFETY: every acquired handle is checked before use, selected objects
        // are restored, and each DC or failed bitmap is released exactly once.
        unsafe
        {
            let screen_dc = GetDC(ptr::null_mut());
            if screen_dc.is_null()
            {
                return None;
            }

            let memory_dc = CreateCompatibleDC(screen_dc);
            if memory_dc.is_null()
            {
                ReleaseDC(ptr::null_mut(), screen_dc);
                return None;
            }

            let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
            if bitmap.is_null()
            {
                DeleteDC(memory_dc);
                ReleaseDC(ptr::null_mut(), screen_dc);
                return None;
            }

            let previous = SelectObject(memory_dc, bitmap);
            let copied = BitBlt(memory_dc, 0, 0, width, height, screen_dc, x, y, SRCCOPY);
            SelectObject(memory_dc, previous);
            DeleteDC(memory_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);

            if copied == 0
            {
                DeleteObject(bitmap);
                return None;
            }

            Some(Screenshot { bitmap, width, height })
        }
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

        // SAFETY: source dimensions were validated, all acquired handles are
        // checked, selected objects are restored, and owned resources are freed.
        unsafe
        {
            let screen_dc = GetDC(ptr::null_mut());
            if screen_dc.is_null()
            {
                return None;
            }

            let source_dc = CreateCompatibleDC(screen_dc);
            let dest_dc = CreateCompatibleDC(screen_dc);
            let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
            ReleaseDC(ptr::null_mut(), screen_dc);

            if source_dc.is_null() || dest_dc.is_null() || bitmap.is_null()
            {
                if !source_dc.is_null()
                {
                    DeleteDC(source_dc);
                }
                if !dest_dc.is_null()
                {
                    DeleteDC(dest_dc);
                }
                if !bitmap.is_null()
                {
                    DeleteObject(bitmap);
                }
                return None;
            }

            let previous_source = SelectObject(source_dc, self.bitmap);
            let previous_dest = SelectObject(dest_dc, bitmap);
            let copied = BitBlt(dest_dc, 0, 0, width, height, source_dc, x, y, SRCCOPY);
            SelectObject(source_dc, previous_source);
            SelectObject(dest_dc, previous_dest);
            DeleteDC(source_dc);
            DeleteDC(dest_dc);

            if copied == 0
            {
                DeleteObject(bitmap);
                return None;
            }

            Some(Screenshot { bitmap, width, height })
        }
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
        // SAFETY: this guard owns `bitmap`; the scratch DC is checked, its
        // original object is restored, and both resources are released once.
        unsafe
        {
            let dc = CreateCompatibleDC(ptr::null_mut());
            if dc.is_null()
            {
                eprintln!("capture: failed to create the wipe DC");
            }
            else
            {
                let previous = SelectObject(dc, self.bitmap);
                if previous.is_null()
                {
                    eprintln!("capture: failed to select the bitmap for wiping");
                }
                else
                {
                    if PatBlt(dc, 0, 0, self.width, self.height, BLACKNESS) == 0
                    {
                        eprintln!("capture: failed to wipe bitmap pixels");
                    }
                    SelectObject(dc, previous);
                }
                if DeleteDC(dc) == 0
                {
                    eprintln!("capture: failed to release the wipe DC");
                }
            }
            if DeleteObject(self.bitmap) == 0
            {
                eprintln!("capture: failed to release the bitmap");
            }
        }
    }
}
