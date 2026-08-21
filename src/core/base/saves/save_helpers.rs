//! Low-level image encoding, circular masking, bitmap extraction, and clipboard
//! conversion used by the save workflow.

use std::ffi::c_void;
use std::ptr;

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, RGBQUAD,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipCreateBitmapFromFile, GdipCreateBitmapFromHBITMAP, GdipCreateBitmapFromScan0,
    GdipCreateHBITMAPFromBitmap, GdipDisposeImage, GdipSaveImageToFile, GpBitmap, GpImage,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::Ole::CF_DIB;

use crate::core::helpers::graphics::gdiplus_helper::{self, GdiPlusToken};
use crate::core::helpers::graphics::screen_capture::Screenshot;

// GDI+ pixel formats for `GdipCreateBitmapFromScan0` (windows-sys does not
// export these constants).
const PIXEL_FORMAT_32BPP_RGB: i32 = 0x0002_2009;
const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;

// CLSIDs of the built-in GDI+ image encoders, which share every field but
// `data1`.
const PNG_ENCODER: GUID = GUID
{
    data1: 0x557C_F406,
    data2: 0x1A04,
    data3: 0x11D3,
    data4: [0x9A, 0x73, 0x00, 0x00, 0xF8, 0x1E, 0xF3, 0x2E],
};

const JPEG_ENCODER: GUID = GUID
{
    data1: 0x557C_F401,
    data2: 0x1A04,
    data3: 0x11D3,
    data4: [0x9A, 0x73, 0x00, 0x00, 0xF8, 0x1E, 0xF3, 0x2E],
};

const BMP_ENCODER: GUID = GUID
{
    data1: 0x557C_F400,
    data2: 0x1A04,
    data3: 0x11D3,
    data4: [0x9A, 0x73, 0x00, 0x00, 0xF8, 0x1E, 0xF3, 0x2E],
};

/// Output image formats exposed by the settings UI, each mapping to a built-in
/// GDI+ encoder and file extension.
pub(super) enum ImageFormat
{
    Png,
    Jpeg,
    Bmp,
}

impl ImageFormat
{
    /// Maps a config `image_format` value to a format, falling back to PNG for
    /// anything unrecognized.
    pub(super) fn from_config(value: &str) -> Self
    {
        match value.trim().to_ascii_lowercase().as_str()
        {
            "jpeg" | "jpg" => Self::Jpeg,
            "bmp" | "bitmap" => Self::Bmp,
            _ => Self::Png,
        }
    }


    /// CLSID of the GDI+ encoder that writes this format.
    pub(super) fn encoder(&self) -> &'static GUID
    {
        match self
        {
            Self::Png => &PNG_ENCODER,
            Self::Jpeg => &JPEG_ENCODER,
            Self::Bmp => &BMP_ENCODER,
        }
    }


    /// File extension, without the dot, for this format.
    pub(super) fn extension(&self) -> &'static str
    {
        match self
        {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Bmp => "bmp",
        }
    }
}


/// Encodes the GDI `bitmap` to `path` via GDI+ using the given encoder CLSID.
/// Returns `true` on success.
pub(super) fn encode_image(bitmap: HBITMAP, path: &str, encoder: &GUID) -> bool
{
    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return false,
    };

    let mut gp_bitmap: *mut GpBitmap = ptr::null_mut();

    // SAFETY: `bitmap` is a valid GDI bitmap and `gp_bitmap` a local out-pointer.
    if unsafe { GdipCreateBitmapFromHBITMAP(bitmap, ptr::null_mut(), &mut gp_bitmap) } != 0 || gp_bitmap.is_null()
    {
        eprintln!("save: failed to wrap bitmap for encoding");
        return false;
    }

    let wide = gdiplus_helper::wide(path);

    // SAFETY: `gp_bitmap` is a live GDI+ bitmap and `wide` is NUL-terminated.
    let status = unsafe { GdipSaveImageToFile(gp_bitmap as *mut GpImage, wide.as_ptr(), encoder, ptr::null()) };

    // SAFETY: `gp_bitmap` was created above and is disposed exactly once.
    unsafe { GdipDisposeImage(gp_bitmap as *mut GpImage) };

    if status != 0
    {
        eprintln!("save: failed to encode image: {path}");
    }

    status == 0
}


/// Encodes `screenshot` to `path` masked to the circle inscribed in its bounds.
/// Uses transparency for PNG and white corners for formats without alpha.
/// Returns `true` on success.
pub(super) fn encode_circular_image(screenshot: &Screenshot, path: &str, format: &ImageFormat) -> bool
{
    let (width, height) = screenshot.dimensions();
    let mut pixels = match bitmap_pixels_bgra(screenshot.bitmap(), width, height)
    {
        Some(pixels) => pixels,
        None => return false,
    };

    let transparent = matches!(format, ImageFormat::Png);
    apply_circle_mask(&mut pixels, width, height, transparent);

    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return false,
    };

    let pixel_format = if transparent { PIXEL_FORMAT_32BPP_ARGB } else { PIXEL_FORMAT_32BPP_RGB };
    let mut gp_bitmap: *mut GpBitmap = ptr::null_mut();

    // SAFETY: `pixels` holds `height` top-down rows of `width * 4` bytes and
    // outlives the bitmap, which reads from it until disposed below.
    if unsafe { GdipCreateBitmapFromScan0(width, height, width * 4, pixel_format, pixels.as_ptr(), &mut gp_bitmap) } != 0 || gp_bitmap.is_null()
    {
        eprintln!("save: failed to build masked bitmap");
        return false;
    }

    let wide = gdiplus_helper::wide(path);

    // SAFETY: `gp_bitmap` is a live GDI+ bitmap and `wide` is NUL-terminated.
    let status = unsafe { GdipSaveImageToFile(gp_bitmap as *mut GpImage, wide.as_ptr(), format.encoder(), ptr::null()) };

    // SAFETY: `gp_bitmap` was created above and is disposed exactly once.
    unsafe { GdipDisposeImage(gp_bitmap as *mut GpImage) };

    if status != 0
    {
        eprintln!("save: failed to encode circular image: {path}");
    }

    status == 0
}


/// Loads the cleaned image at `path` and places it on the Windows clipboard as
/// a 24-bpp DIB, flattening any transparency onto white. Returns `true` on success.
pub(super) fn copy_image_to_clipboard(path: &str) -> bool
{
    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return false,
    };

    let wide = gdiplus_helper::wide(path);
    let mut gp_bitmap: *mut GpBitmap = ptr::null_mut();

    // SAFETY: `wide` is NUL-terminated and `gp_bitmap` a local out-pointer.
    if unsafe { GdipCreateBitmapFromFile(wide.as_ptr(), &mut gp_bitmap) } != 0 || gp_bitmap.is_null()
    {
        eprintln!("save: failed to load image for clipboard: {path}");
        return false;
    }

    let mut hbitmap: HBITMAP = ptr::null_mut();

    // SAFETY: `gp_bitmap` is a live GDI+ bitmap and `hbitmap` a local out-pointer.
    let status = unsafe { GdipCreateHBITMAPFromBitmap(gp_bitmap, &mut hbitmap, 0xFFFF_FFFF) };

    // SAFETY: `gp_bitmap` was created above and is disposed exactly once.
    unsafe { GdipDisposeImage(gp_bitmap as *mut GpImage) };

    if status != 0 || hbitmap.is_null()
    {
        eprintln!("save: failed to convert image for clipboard: {path}");
        return false;
    }

    let copied = dib_to_clipboard(hbitmap);

    // SAFETY: `hbitmap` is owned here; the clipboard received a copy, not this handle.
    unsafe { DeleteObject(hbitmap) };

    copied
}


/// Extracts `hbitmap`'s pixels as top-down 32-bpp BGRA rows. Returns `None`
/// when the size is non-positive or the bits cannot be read.
fn bitmap_pixels_bgra(hbitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>>
{
    if width <= 0 || height <= 0
    {
        eprintln!("save: bitmap has invalid dimensions");
        return None;
    }

    let header = BITMAPINFOHEADER
    {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let mut info = BITMAPINFO { bmiHeader: header, bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }] };

    let byte_count = match (width as usize).checked_mul(height as usize).and_then(|pixels| pixels.checked_mul(4))
    {
        Some(byte_count) => byte_count,
        None =>
        {
            eprintln!("save: bitmap dimensions exceed addressable memory");
            return None;
        }
    };

    let mut pixels = Vec::new();
    if pixels.try_reserve_exact(byte_count).is_err()
    {
        eprintln!("save: failed to allocate the bitmap pixel buffer");
        return None;
    }
    pixels.resize(byte_count, 0);

    // SAFETY: the screen DC is checked and released once; `pixels` and `info`
    // remain live and describe exactly the requested 32-bpp output buffer.
    unsafe
    {
        let screen_dc = GetDC(ptr::null_mut());
        if screen_dc.is_null()
        {
            eprintln!("save: failed to acquire a screen DC");
            return None;
        }

        let extracted = GetDIBits(screen_dc, hbitmap, 0, height as u32, pixels.as_mut_ptr() as *mut c_void, &mut info, DIB_RGB_COLORS);
        ReleaseDC(ptr::null_mut(), screen_dc);

        if extracted == 0
        {
            eprintln!("save: failed to read bitmap pixels");
            return None;
        }

        Some(pixels)
    }
}


/// Masks BGRA `pixels` to the largest circle inscribed in `width`×`height`.
/// Outside pixels become transparent when requested and white otherwise.
fn apply_circle_mask(pixels: &mut [u8], width: i32, height: i32, transparent: bool)
{
    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;
    let radius = center_x.min(center_y);
    let radius_squared = radius * radius;

    for y in 0..height
    {
        for x in 0..width
        {
            let dx = x as f64 + 0.5 - center_x;
            let dy = y as f64 + 0.5 - center_y;
            let offset = ((y * width + x) * 4) as usize;

            if dx * dx + dy * dy > radius_squared
            {
                if transparent
                {
                    pixels[offset] = 0;
                    pixels[offset + 1] = 0;
                    pixels[offset + 2] = 0;
                    pixels[offset + 3] = 0;
                }
                else
                {
                    pixels[offset] = 255;
                    pixels[offset + 1] = 255;
                    pixels[offset + 2] = 255;
                }
            }
            else
            {
                pixels[offset + 3] = 255;
            }
        }
    }
}


/// Converts `hbitmap` into a packed 24-bpp DIB and hands it to the clipboard.
/// Returns `true` on success.
fn dib_to_clipboard(hbitmap: HBITMAP) -> bool
{
    // SAFETY: `BITMAP` is a plain Win32 data structure valid when zeroed.
    let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };

    // SAFETY: `bitmap` is a local sized to `BITMAP`, filled by GetObjectW.
    if unsafe { GetObjectW(hbitmap, std::mem::size_of::<BITMAP>() as i32, &mut bitmap as *mut BITMAP as *mut c_void) } == 0
    {
        eprintln!("save: failed to query clipboard bitmap");
        return false;
    }

    let width = bitmap.bmWidth;
    let height = bitmap.bmHeight;
    if width <= 0 || height <= 0
    {
        eprintln!("save: clipboard bitmap has invalid dimensions");
        return false;
    }

    let stride = match (width as usize).checked_mul(24).and_then(|bits| bits.checked_add(31)).and_then(|bits| (bits / 32).checked_mul(4))
    {
        Some(stride) => stride,
        None =>
        {
            eprintln!("save: clipboard row size exceeds addressable memory");
            return false;
        }
    };
    let image_size = match stride.checked_mul(height as usize)
    {
        Some(image_size) => image_size,
        None =>
        {
            eprintln!("save: clipboard image exceeds addressable memory");
            return false;
        }
    };
    let image_size_u32 = match u32::try_from(image_size)
    {
        Ok(image_size) => image_size,
        Err(_) =>
        {
            eprintln!("save: clipboard image exceeds the DIB size limit");
            return false;
        }
    };
    let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let allocation_size = match header_size.checked_add(image_size)
    {
        Some(allocation_size) => allocation_size,
        None =>
        {
            eprintln!("save: clipboard allocation size overflowed");
            return false;
        }
    };

    let header = BITMAPINFOHEADER
    {
        biSize: header_size as u32,
        biWidth: width,
        biHeight: height,
        biPlanes: 1,
        biBitCount: 24,
        biCompression: BI_RGB,
        biSizeImage: image_size_u32,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };

    // SAFETY: the DC and movable allocation are checked and released on every
    // failure path; `dest` spans the header and pixel sizes used by both writes.
    unsafe
    {
        let screen_dc = GetDC(ptr::null_mut());
        if screen_dc.is_null()
        {
            eprintln!("save: failed to acquire a clipboard conversion DC");
            return false;
        }

        let hmem = GlobalAlloc(GMEM_MOVEABLE, allocation_size);
        if hmem.is_null()
        {
            eprintln!("save: failed to allocate clipboard memory");
            ReleaseDC(ptr::null_mut(), screen_dc);
            return false;
        }

        let dest = GlobalLock(hmem) as *mut u8;
        if dest.is_null()
        {
            eprintln!("save: failed to lock clipboard memory");
            GlobalFree(hmem);
            ReleaseDC(ptr::null_mut(), screen_dc);
            return false;
        }

        ptr::copy_nonoverlapping(&header as *const BITMAPINFOHEADER as *const u8, dest, header_size);
        let extracted = GetDIBits(screen_dc, hbitmap, 0, height as u32, dest.add(header_size) as *mut c_void, dest as *mut BITMAPINFO, DIB_RGB_COLORS);

        GlobalUnlock(hmem);
        ReleaseDC(ptr::null_mut(), screen_dc);

        if extracted == 0
        {
            eprintln!("save: failed to extract clipboard pixels");
            GlobalFree(hmem);
            return false;
        }

        set_clipboard_dib(hmem)
    }
}


/// Replaces the clipboard's contents with the DIB block `hmem`. On success the
/// system takes ownership; on failure it is freed here. Returns `true` on success.
fn set_clipboard_dib(hmem: *mut c_void) -> bool
{
    // SAFETY: `hmem` is a valid GlobalAlloc block; it is either handed to the
    // clipboard (which then owns it) or freed here, never both.
    unsafe
    {
        if OpenClipboard(ptr::null_mut()) == 0
        {
            eprintln!("save: failed to open clipboard");
            GlobalFree(hmem);
            return false;
        }

        if EmptyClipboard() == 0
        {
            eprintln!("save: failed to empty the clipboard");
            CloseClipboard();
            GlobalFree(hmem);
            return false;
        }
        let handle = SetClipboardData(CF_DIB as u32, hmem);
        if CloseClipboard() == 0
        {
            eprintln!("save: failed to close the clipboard");
        }

        if handle.is_null()
        {
            eprintln!("save: failed to set clipboard data");
            GlobalFree(hmem);
            return false;
        }
    }

    true
}
