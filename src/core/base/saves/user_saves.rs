//! Saving captured images as cleaned files in the working directory's `Images` or user-configured save
//! folder, in the format (PNG, JPEG, or BMP) chosen in the settings UI, and
//! copying the cleaned image to the Windows clipboard.




use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipCreateBitmapFromFile, GdipCreateBitmapFromHBITMAP, GdipCreateHBITMAPFromBitmap,
    GdipDisposeImage, GdipSaveImageToFile, GdiplusShutdown, GdiplusStartup, GdiplusStartupInput,
    GpBitmap, GpImage,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::Ole::CF_DIB;



use crate::core::base::configs::config_master::{self, Config};
use crate::core::base::notify::notifications_handler;
use crate::core::helpers::file_data_operations::metadata;
use crate::core::helpers::file_data_operations::xif_data;
use crate::core::helpers::file_operations::file_helper;
use crate::core::helpers::graphics::screen_capture::Screenshot;

const IMAGES_DIR: &str = "Images";


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


/// Output image formats the settings UI exposes, each mapping to a built-in
/// GDI+ encoder and file extension.
enum ImageFormat
{
    Png,
    Jpeg,
    Bmp,
}

impl ImageFormat
{
    /// Maps a config `image_format` value to a format, falling back to PNG for
    /// anything unrecognized.
    fn from_config(value: &str) -> Self
    {
        match value.trim().to_ascii_lowercase().as_str()
        {
            "jpeg" | "jpg" => Self::Jpeg,
            "bmp" | "bitmap" => Self::Bmp,
            _ => Self::Png,
        }
    }

    /// CLSID of the GDI+ encoder that writes this format.
    fn encoder(&self) -> &'static GUID
    {
        match self
        {
            Self::Png => &PNG_ENCODER,
            Self::Jpeg => &JPEG_ENCODER,
            Self::Bmp => &BMP_ENCODER,
        }
    }

    /// File extension, without the dot, for this format.
    fn extension(&self) -> &'static str
    {
        match self
        {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Bmp => "bmp",
        }
    }
}


/// Writes `screenshot` as a cleaned image, then applies the user's chosen
/// disposition: copy it to the clipboard, keep it on disk, or both. The file is
/// always written first so the clipboard copy reads from the metadata-stripped
/// result; when auto-save is off it is removed once copied. Returns the saved
/// path when the file is kept, otherwise `None`.
pub fn save_screenshot(screenshot: &Screenshot) -> Option<PathBuf>
{
    let config = config_master::load_config();
    let copy_to_clipboard = config.as_ref().map(|config| config.copy_to_clipboard).unwrap_or(false);
    let auto_save = config.as_ref().map(|config| config.auto_save).unwrap_or(true);

    let directory = target_directory(config.as_ref())?;

    if !file_helper::create_directory(&directory)
    {
        return None;
    }

    let format = configured_format(config.as_ref());
    let file_name = format!("{}.{}", file_helper::random_string(), format.extension());
    let path = Path::new(&directory).join(file_name);
    let path_text = path.to_string_lossy().into_owned();

    if !encode_image(screenshot.bitmap(), &path_text, format.encoder())
    {
        return None;
    }

    let _ = xif_data::strip_exif(&path_text);
    let _ = metadata::strip_metadata(&path_text);
    apply_custom_data(&path_text, config.as_ref());

    if copy_to_clipboard
    {
        copy_image_to_clipboard(&path_text);
    }

    if !auto_save
    {
        let _ = std::fs::remove_file(&path);
        if copy_to_clipboard
        {
            notifications_handler::notify_screenshot_clipboardsaved(None);
        }
        return None;
    }

    if copy_to_clipboard
    {
        notifications_handler::notify_screenshot_clipboardsaved(Some(&path));
    }
    else
    {
        notifications_handler::notify_screenshot_saved(&path);
    }

    Some(path)
}


/// Writes configured custom image data when the user has enabled replacements.
fn apply_custom_data(path: &str, config: Option<&Config>)
{
    let Some(config) = config
    else
    {
        return;
    };

    if !config.fill_custom_data
    {
        return;
    }

    let exif_value = config.custom_data.exif.trim();
    if !exif_value.is_empty()
    {
        let _ = xif_data::write_custom_exif(path, exif_value);
    }

    let metadata_value = config.custom_data.metadata.trim();
    if !metadata_value.is_empty()
    {
        let metadata = metadata_from_value(metadata_value);
        let _ = metadata::write_metadata(path, &metadata);
    }
}


/// Builds a metadata payload whose common tags all carry `value`.
fn metadata_from_value(value: &str) -> metadata::Metadata
{
    let value = value.to_string();

    metadata::Metadata
    {
        document_name: Some(value.clone()),
        description: Some(value.clone()),
        software: Some(value.clone()),
        date_time: Some(value.clone()),
        artist: Some(value.clone()),
        host_computer: Some(value.clone()),
        copyright: Some(value.clone()),
        title: Some(value.clone()),
        comment: Some(value.clone()),
        author: Some(value.clone()),
        keywords: Some(value.clone()),
        subject: Some(value),
    }
}


/// Resolves the directory to save into: the user's configured `save_directory`
/// when set, otherwise the built-in `<working_dir>/Images` fallback.
fn target_directory(config: Option<&Config>) -> Option<String>
{
    if let Some(config) = config
    {
        let directory = config.save_directory.trim();
        if !directory.is_empty()
        {
            return Some(directory.to_string());
        }
    }

    Some(images_dir()?.to_string_lossy().into_owned())
}


/// Returns `<working_dir>/Images`, mirroring config_master's directory logic.
fn images_dir() -> Option<PathBuf>
{
    let mut directory = std::env::current_dir().ok()?;
    directory.push(IMAGES_DIR);
    Some(directory)
}


/// Reads the user's configured output format, defaulting to PNG when no config
/// has been saved yet.
fn configured_format(config: Option<&Config>) -> ImageFormat
{
    match config
    {
        Some(config) => ImageFormat::from_config(&config.image_format),
        None => ImageFormat::Png,
    }
}


/// Encodes the GDI bitmap to `path` via GDI+ using the given encoder CLSID.
fn encode_image(bitmap: HBITMAP, path: &str, encoder: &GUID) -> bool
{
    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return false,
    };

    let mut gp_bitmap: *mut GpBitmap = ptr::null_mut();

    if unsafe { GdipCreateBitmapFromHBITMAP(bitmap, ptr::null_mut(), &mut gp_bitmap) } != 0
        || gp_bitmap.is_null()
    {
        return false;
    }

    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();

    let status = unsafe {
        GdipSaveImageToFile(gp_bitmap as *mut GpImage, wide.as_ptr(), encoder, ptr::null())
    };

    unsafe { GdipDisposeImage(gp_bitmap as *mut GpImage) };

    status == 0
}


fn copy_image_to_clipboard(path: &str) -> bool
{
    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return false,
    };

    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();

    let mut gp_bitmap: *mut GpBitmap = ptr::null_mut();

    if unsafe { GdipCreateBitmapFromFile(wide.as_ptr(), &mut gp_bitmap) } != 0
        || gp_bitmap.is_null()
    {
        return false;
    }

    let mut hbitmap: HBITMAP = ptr::null_mut();

    let status = unsafe { GdipCreateHBITMAPFromBitmap(gp_bitmap, &mut hbitmap, 0xFF00_0000) };

    unsafe { GdipDisposeImage(gp_bitmap as *mut GpImage) };

    if status != 0 || hbitmap.is_null()
    {
        return false;
    }

    let copied = dib_to_clipboard(hbitmap);

    unsafe { DeleteObject(hbitmap) };

    copied
}


/// Converts `hbitmap` into a packed 24-bpp DIB and hands it to the clipboard.
/// 24-bpp (no alpha channel) keeps apps from rendering the opaque screenshot as
/// transparent. Returns `true` on success.
fn dib_to_clipboard(hbitmap: HBITMAP) -> bool
{
    let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };

    if unsafe {
        GetObjectW(
            hbitmap as *mut c_void,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bitmap as *mut BITMAP as *mut c_void,
        )
    } == 0
    {
        return false;
    }

    let width = bitmap.bmWidth;
    let height = bitmap.bmHeight;
    if width <= 0 || height <= 0
    {
        return false;
    }

    let stride = (((width * 24) + 31) / 32) * 4;
    let image_size = (stride * height) as usize;
    let header_size = std::mem::size_of::<BITMAPINFOHEADER>();

    let header = BITMAPINFOHEADER
    {
        biSize: header_size as u32,
        biWidth: width,
        biHeight: height,
        biPlanes: 1,
        biBitCount: 24,
        biCompression: BI_RGB as u32,
        biSizeImage: image_size as u32,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };

    let screen_dc = unsafe { GetDC(ptr::null_mut()) };
    if screen_dc.is_null()
    {
        return false;
    }

    let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, header_size + image_size) };
    if hmem.is_null()
    {
        unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };
        return false;
    }

    let dest = unsafe { GlobalLock(hmem) } as *mut u8;
    if dest.is_null()
    {
        unsafe { GlobalFree(hmem) };
        unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };
        return false;
    }

    let extracted = unsafe {
        ptr::copy_nonoverlapping(&header as *const BITMAPINFOHEADER as *const u8, dest, header_size);
        GetDIBits(
            screen_dc,
            hbitmap,
            0,
            height as u32,
            dest.add(header_size) as *mut c_void,
            dest as *mut BITMAPINFO,
            DIB_RGB_COLORS,
        )
    };

    unsafe { GlobalUnlock(hmem) };
    unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };

    if extracted == 0
    {
        unsafe { GlobalFree(hmem) };
        return false;
    }

    set_clipboard_dib(hmem)
}


/// Replaces the clipboard's contents with the DIB block `hmem`. On success the
/// system takes ownership of `hmem`; on any failure it is freed here. Returns
/// `true` on success.
fn set_clipboard_dib(hmem: *mut c_void) -> bool
{
    unsafe {
        
        if OpenClipboard(ptr::null_mut()) == 0
        {
            GlobalFree(hmem);
            return false;
        }

        EmptyClipboard();
        let handle = SetClipboardData(CF_DIB as u32, hmem);
        CloseClipboard();

        if handle.is_null()
        {
            GlobalFree(hmem);
            return false;
        }
    }

    true
}


struct GdiPlusToken
{
    token: usize,
}

impl GdiPlusToken
{
    fn startup() -> Option<Self>
    {
        let input = GdiplusStartupInput
        {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };

        let mut token: usize = 0;

        if unsafe { GdiplusStartup(&mut token, &input, ptr::null_mut()) } != 0
        {
            return None;
        }

        Some(Self { token })
    }
}

impl Drop for GdiPlusToken
{
    fn drop(&mut self)
    {
        unsafe { GdiplusShutdown(self.token) };
    }
}
