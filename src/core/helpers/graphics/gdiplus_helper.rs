//! Shared GDI+ plumbing used by the image cleaning and saving modules: RAII
//! guards for GDI+ startup and loaded images, property-item access, and the
//! temp-file save/rename helpers.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::core::GUID;
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipDisposeImage, GdipGetImageEncoders, GdipGetImageEncodersSize, GdipGetImageRawFormat,
    GdipGetPropertyItem, GdipGetPropertyItemSize, GdipLoadImageFromFile, GdipSaveImageToFile,
    GdiplusShutdown, GdiplusStartup, GdiplusStartupInput, GpImage, ImageCodecInfo, PropertyItem,
};

use crate::core::helpers::file_operations::file_helper;

/// RAII guard that initializes GDI+ on construction and shuts it down on drop.
pub struct GdiPlusToken
{
    token: usize,
}


impl GdiPlusToken
{
    /// Starts a GDI+ session and returns a guard that shuts it down when
    /// dropped, or `None` if startup fails.
    pub fn startup() -> Option<Self>
    {
        let input = GdiplusStartupInput
        {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token: usize = 0;

        // SAFETY: `token` and `input` are locals that outlive the call.
        if unsafe { GdiplusStartup(&mut token, &input, ptr::null_mut()) } != 0
        {
            eprintln!("gdiplus: startup failed");
            return None;
        }

        Some(Self { token })
    }
}


impl Drop for GdiPlusToken
{
    /// Shuts down the GDI+ session opened by [`GdiPlusToken::startup`].
    fn drop(&mut self)
    {
        // SAFETY: `token` came from a successful `GdiplusStartup` call.
        unsafe { GdiplusShutdown(self.token) };
    }
}


/// RAII guard wrapping a loaded GDI+ image, disposed on drop.
pub struct LoadedImage
{
    pub handle: *mut GpImage,
}


impl LoadedImage
{
    /// Loads the image at `path` into GDI+, or `None` when it cannot be opened
    /// as an image.
    pub fn load(path: &str) -> Option<Self>
    {
        let wide = wide(path);
        let mut handle: *mut GpImage = ptr::null_mut();

        // SAFETY: `wide` is a NUL-terminated buffer and `handle` a local out-pointer.
        if unsafe { GdipLoadImageFromFile(wide.as_ptr(), &mut handle) } != 0 || handle.is_null()
        {
            eprintln!("gdiplus: failed to load image: {path}");
            return None;
        }

        Some(Self { handle })
    }
}


impl Drop for LoadedImage
{
    /// Disposes the GDI+ image owned by this guard.
    fn drop(&mut self)
    {
        // SAFETY: `handle` is the non-null image this guard owns.
        unsafe { GdipDisposeImage(self.handle) };
    }
}


/// Encodes a string as a NUL-terminated UTF-16 buffer for the Win32 `*W` APIs.
pub fn wide(value: &str) -> Vec<u16>
{
    OsStr::new(value).encode_wide().chain(std::iter::once(0)).collect()
}


/// Fetches one property item from `image`, returning its value type and raw
/// value bytes, or `None` when the tag is absent or unreadable.
pub fn get_property(image: *mut GpImage, propid: u32) -> Option<(u16, Vec<u8>)>
{
    let mut size: u32 = 0;

    // SAFETY: `size` is a local out-parameter for the item's byte length.
    if unsafe { GdipGetPropertyItemSize(image, propid, &mut size) } != 0
    {
        return None;
    }

    if (size as usize) < std::mem::size_of::<PropertyItem>()
    {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];

    // SAFETY: `buffer` is exactly `size` bytes, as GDI+ requires for the item.
    if unsafe { GdipGetPropertyItem(image, propid, size, buffer.as_mut_ptr() as *mut PropertyItem) } != 0
    {
        return None;
    }

    // SAFETY: GDI+ wrote a PropertyItem at the start of `buffer`; the unaligned
    // read copies it out, and `value`/`length` describe bytes inside `buffer`.
    let item = unsafe { ptr::read_unaligned(buffer.as_ptr() as *const PropertyItem) };

    if item.value.is_null()
    {
        return None;
    }

    // SAFETY: `item.value` points at `item.length` live bytes owned by `buffer`.
    let value = unsafe { std::slice::from_raw_parts(item.value as *const u8, item.length as usize) };

    Some((item.r#type, value.to_vec()))
}


/// Re-encodes `image` over `path` via a temp file and atomic rename, choosing
/// the encoder that matches the image's own format. Consumes `image` so its
/// lock on the original file is released before the rename. Returns `true` on
/// success.
pub fn save_over(path: &str, image: LoadedImage) -> bool
{
    let mut clsid = GUID { data1: 0, data2: 0, data3: 0, data4: [0; 8] };

    if !find_encoder(image.handle, &mut clsid)
    {
        return false;
    }

    let temp = temp_path(path);
    let wide = wide(&temp);

    // SAFETY: `wide` is NUL-terminated and `clsid` outlives the call.
    let status = unsafe { GdipSaveImageToFile(image.handle, wide.as_ptr(), &clsid, ptr::null()) };

    if status != 0
    {
        eprintln!("gdiplus: failed to save image: {temp}");

        let _ = std::fs::remove_file(&temp);

        return false;
    }

    drop(image);

    commit_rename(&temp, path)
}


/// Builds a unique sibling temp path next to `path`, on the same volume.
pub fn temp_path(path: &str) -> String
{
    format!("{path}.{}.tmp", file_helper::random_string())
}


/// Renames `temp` onto `path`, deleting `temp` if the replace fails. Returns
/// `true` on success.
pub fn commit_rename(temp: &str, path: &str) -> bool
{
    if std::fs::rename(temp, path).is_ok()
    {
        true
    }
    else
    {
        eprintln!("gdiplus: failed to replace {path} with {temp}");

        let _ = std::fs::remove_file(temp);

        false
    }
}


/// Finds the encoder CLSID matching the image's raw format, writing it to
/// `clsid`. Returns `true` when a matching encoder exists.
fn find_encoder(image: *mut GpImage, clsid: &mut GUID) -> bool
{
    let mut format = GUID { data1: 0, data2: 0, data3: 0, data4: [0; 8] };

    // SAFETY: `format` is a local out-parameter.
    if unsafe { GdipGetImageRawFormat(image, &mut format) } != 0
    {
        return false;
    }

    let mut num: u32 = 0;
    let mut size: u32 = 0;

    // SAFETY: `num` and `size` are local out-parameters.
    if unsafe { GdipGetImageEncodersSize(&mut num, &mut size) } != 0 || num == 0 || size == 0
    {
        return false;
    }

    let mut buffer = vec![0u8; size as usize];

    // SAFETY: `buffer` is exactly `size` bytes, as GDI+ requires for the list.
    if unsafe { GdipGetImageEncoders(num, size, buffer.as_mut_ptr() as *mut ImageCodecInfo) } != 0
    {
        return false;
    }

    let stride = std::mem::size_of::<ImageCodecInfo>();
    
    for i in 0..num as usize
    {
        // SAFETY: `buffer` holds `num` codec entries; the unaligned read copies one out.
        let codec = unsafe { ptr::read_unaligned(buffer.as_ptr().add(i * stride) as *const ImageCodecInfo) };

        if guid_eq(&codec.FormatID, &format)
        {
            *clsid = codec.Clsid;
            return true;
        }
    }

    eprintln!("gdiplus: no encoder matches the image format");

    false
}


/// Compares two GUIDs field by field (windows-sys `GUID` is not `PartialEq`).
fn guid_eq(a: &GUID, b: &GUID) -> bool
{
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}
