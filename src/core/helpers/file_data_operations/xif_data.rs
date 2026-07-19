//! Stripping and writing EXIF metadata in photo files via GDI+.

use std::ffi::c_void;

use windows_sys::Win32::Graphics::GdiPlus::{
    GdipGetPropertyCount, GdipGetPropertyIdList, GdipRemovePropertyItem, GdipSetPropertyItem,
    GpImage, PropertyItem,
};

use crate::core::helpers::file_operations::file_helper;
use crate::core::helpers::graphics::gdiplus_helper::{self, GdiPlusToken, LoadedImage};

// EXIF tag identifiers as exposed by GDI+ property items.
const TAG_MAKE: u32 = 0x010F;
const TAG_MODEL: u32 = 0x0110;
const TAG_SOFTWARE: u32 = 0x0131;
const TAG_DATE_TIME_ORIGINAL: u32 = 0x9003;

// EXIF/GDI+ property value types.
const TYPE_ASCII: u16 = 2;

/// Outcome of [`strip_exif`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StripExifResult
{
    /// The image could not be read, parsed, or written.
    Failed = 0,
    /// The image had no EXIF metadata; the file was left untouched.
    NoExifFound = 1,
    /// EXIF metadata was found and removed, and the file was rewritten in place.
    Stripped = 2,
}


/// Removes EXIF metadata from the photo at `path`, overwriting it in place via a
/// temp file and atomic rename. JPEG files are stripped losslessly; other
/// formats are re-encoded through GDI+. Returns [`StripExifResult::NoExifFound`]
/// when there is nothing to remove.
pub fn strip_exif(path: &str) -> StripExifResult
{
    if file_helper::is_jpeg(path)
    {
        strip_jpeg_exif(path)
    }
    else
    {
        strip_via_gdiplus(path)
    }
}


/// Writes the replacement `value` to the text-based EXIF tags (make, model,
/// software, and original date/time) of the image at `path`, overwriting it in
/// place. Returns `true` when at least one tag was written and the file saved.
pub fn write_custom_exif(path: &str, value: &str) -> bool
{
    let value = value.trim();
    if value.is_empty()
    {
        return false;
    }

    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return false,
    };
    let image = match LoadedImage::load(path)
    {
        Some(image) => image,
        None => return false,
    };

    let mut applied = false;
    applied |= set_ascii_property(image.handle, TAG_MAKE, value);
    applied |= set_ascii_property(image.handle, TAG_MODEL, value);
    applied |= set_ascii_property(image.handle, TAG_SOFTWARE, value);
    applied |= set_ascii_property(image.handle, TAG_DATE_TIME_ORIGINAL, value);

    if !applied
    {
        return false;
    }

    gdiplus_helper::save_over(path, image)
}


/// Rewrites a JPEG at `path` without its EXIF APP1 segment(s), preserving every
/// other segment and the entropy-coded image data byte-for-byte (no
/// recompression). Returns the strip outcome.
fn strip_jpeg_exif(path: &str) -> StripExifResult
{
    let data = match std::fs::read(path)
    {
        Ok(data) => data,
        Err(_) =>
        {
            eprintln!("xif: failed to read JPEG: {path}");
            return StripExifResult::Failed;
        }
    };
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8
    {
        eprintln!("xif: not a JPEG stream: {path}");
        return StripExifResult::Failed;
    }

    let mut output: Vec<u8> = Vec::with_capacity(data.len());
    output.extend_from_slice(&data[0..2]);
    let mut pos = 2usize;
    let mut removed = false;

    while pos + 1 < data.len()
    {
        if data[pos] != 0xFF
        {
            eprintln!("xif: malformed JPEG segment: {path}");
            return StripExifResult::Failed;
        }

        let segment_start = pos;
        while pos < data.len() && data[pos] == 0xFF
        {
            pos += 1;
        }
        if pos >= data.len()
        {
            eprintln!("xif: truncated JPEG: {path}");
            return StripExifResult::Failed;
        }
        let marker = data[pos];
        pos += 1;

        if marker == 0xDA
        {
            output.extend_from_slice(&data[segment_start..]);
            return finish_jpeg(path, &output, removed);
        }

        if marker == 0x01 || (0xD0..=0xD9).contains(&marker)
        {
            output.extend_from_slice(&data[segment_start..pos]);
            continue;
        }

        if pos + 1 >= data.len()
        {
            eprintln!("xif: truncated JPEG: {path}");
            return StripExifResult::Failed;
        }
        let length = ((data[pos] as usize) << 8) | data[pos + 1] as usize;
        if length < 2 || pos + length > data.len()
        {
            eprintln!("xif: malformed JPEG segment length: {path}");
            return StripExifResult::Failed;
        }
        let segment_end = pos + length;

        let payload = &data[pos + 2..segment_end];
        let is_exif = marker == 0xE1 && payload.len() >= 6 && &payload[0..6] == b"Exif\0\0";
        if is_exif
        {
            removed = true;
        }
        else
        {
            output.extend_from_slice(&data[segment_start..segment_end]);
        }

        pos = segment_end;
    }

    finish_jpeg(path, &output, removed)
}


/// Commits stripped JPEG bytes to `path` via a temp file and atomic rename, or
/// reports that nothing was removed.
fn finish_jpeg(path: &str, output: &[u8], removed: bool) -> StripExifResult
{
    if !removed
    {
        return StripExifResult::NoExifFound;
    }

    let temp = gdiplus_helper::temp_path(path);
    if std::fs::write(&temp, output).is_err()
    {
        eprintln!("xif: failed to write temp file: {temp}");
        let _ = std::fs::remove_file(&temp);
        return StripExifResult::Failed;
    }

    if gdiplus_helper::commit_rename(&temp, path)
    {
        StripExifResult::Stripped
    }
    else
    {
        StripExifResult::Failed
    }
}


/// Removes every GDI+ property item from a non-JPEG image at `path` and
/// re-encodes it in place. Returns the strip outcome.
fn strip_via_gdiplus(path: &str) -> StripExifResult
{
    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return StripExifResult::Failed,
    };
    let image = match LoadedImage::load(path)
    {
        Some(image) => image,
        None => return StripExifResult::Failed,
    };

    let ids = match property_ids(image.handle)
    {
        Some(ids) => ids,
        None => return StripExifResult::Failed,
    };
    if ids.is_empty()
    {
        return StripExifResult::NoExifFound;
    }

    for id in &ids
    {
        // SAFETY: `image.handle` is a live GDI+ image owned by `image`.
        unsafe { GdipRemovePropertyItem(image.handle, *id) };
    }

    if gdiplus_helper::save_over(path, image)
    {
        StripExifResult::Stripped
    }
    else
    {
        StripExifResult::Failed
    }
}


/// Writes one ASCII property item onto `image`, filtering bytes that are not
/// printable ASCII. Returns `true` when the item was set.
fn set_ascii_property(image: *mut GpImage, propid: u32, value: &str) -> bool
{
    let mut bytes: Vec<u8> = value.bytes().filter(|byte| byte.is_ascii() && !byte.is_ascii_control()).collect();

    if bytes.is_empty()
    {
        return false;
    }

    bytes.push(0);

    let item = PropertyItem
    {
        id: propid,
        length: bytes.len() as u32,
        r#type: TYPE_ASCII,
        value: bytes.as_mut_ptr() as *mut c_void,
    };

    // SAFETY: `item.value` points into `bytes`, which outlives the call.
    unsafe { GdipSetPropertyItem(image, &item) == 0 }
}


/// Returns the list of GDI+ property-item identifiers present on `image`, or
/// `None` when the list cannot be read.
fn property_ids(image: *mut GpImage) -> Option<Vec<u32>>
{
    let mut count: u32 = 0;

    // SAFETY: `count` is a local out-parameter.
    if unsafe { GdipGetPropertyCount(image, &mut count) } != 0
    {
        eprintln!("xif: failed to read property count");
        return None;
    }
    if count == 0
    {
        return Some(Vec::new());
    }

    let mut ids = vec![0u32; count as usize];

    // SAFETY: `ids` holds exactly `count` entries for GDI+ to fill.
    if unsafe { GdipGetPropertyIdList(image, count, ids.as_mut_ptr()) } != 0
    {
        eprintln!("xif: failed to read property id list");
        return None;
    }

    Some(ids)
}
