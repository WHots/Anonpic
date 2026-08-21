//! Removing and writing common document/authoring metadata in image files via
//! GDI+. This complements `xif_data`, which focuses on camera and GPS EXIF;
//! here the focus is the identity-revealing authoring fields (artist,
//! copyright, software, host machine, and the Windows "Details" tab tags).

use std::ffi::c_void;

use windows_sys::Win32::Graphics::GdiPlus::{
    GdipRemovePropertyItem, GdipSetPropertyItem, GpImage, PropertyItem,
};

use crate::core::helpers::graphics::gdiplus_helper::{self, GdiPlusToken, LoadedImage};

// TIFF/EXIF authoring tags carried as NUL-terminated ASCII.
const TAG_DOCUMENT_NAME: u32 = 0x010D;
const TAG_IMAGE_DESCRIPTION: u32 = 0x010E;
const TAG_SOFTWARE: u32 = 0x0131;
const TAG_DATE_TIME: u32 = 0x0132;
const TAG_ARTIST: u32 = 0x013B;
const TAG_HOST_COMPUTER: u32 = 0x013C;
const TAG_COPYRIGHT: u32 = 0x8298;

// Windows XP "Details" tags, carried as NUL-terminated little-endian UTF-16.
const TAG_XP_TITLE: u32 = 0x9C9B;
const TAG_XP_COMMENT: u32 = 0x9C9C;
const TAG_XP_AUTHOR: u32 = 0x9C9D;
const TAG_XP_KEYWORDS: u32 = 0x9C9E;
const TAG_XP_SUBJECT: u32 = 0x9C9F;

// Every modeled tag, used when stripping all common metadata at once.
const COMMON_TAGS: [u32; 12] =
[
    TAG_DOCUMENT_NAME,
    TAG_IMAGE_DESCRIPTION,
    TAG_SOFTWARE,
    TAG_DATE_TIME,
    TAG_ARTIST,
    TAG_HOST_COMPUTER,
    TAG_COPYRIGHT,
    TAG_XP_TITLE,
    TAG_XP_COMMENT,
    TAG_XP_AUTHOR,
    TAG_XP_KEYWORDS,
    TAG_XP_SUBJECT,
];

// GDI+ property value types used by the modeled tags.
const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;

/// The common authoring metadata points carried by an image. Every field is
/// optional because any individual tag may be absent. The first group are
/// general TIFF/EXIF authoring tags; `title` through `subject` are the Windows
/// "Details" tab tags (XP* tags).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata
{
    pub document_name: Option<String>,
    pub description: Option<String>,
    pub software: Option<String>,
    pub date_time: Option<String>,
    pub artist: Option<String>,
    pub host_computer: Option<String>,
    pub copyright: Option<String>,
    pub title: Option<String>,
    pub comment: Option<String>,
    pub author: Option<String>,
    pub keywords: Option<String>,
    pub subject: Option<String>,
}

/// Outcome of [`strip_metadata`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StripMetadataResult
{
    /// The image could not be read, edited, or written.
    Failed = 0,
    /// The image carried none of the modeled tags; the file was left untouched.
    NoMetadataFound = 1,
    /// Modeled tags were found and removed, and the file was rewritten in place.
    Stripped = 2,
}


/// Removes every modeled metadata tag from the image at `path`, overwriting it
/// in place via a temp file and atomic rename. Because GDI+ has no lossless edit
/// path, the image is re-encoded; persistence depends on the format's encoder.
/// Returns [`StripMetadataResult::NoMetadataFound`] when nothing was present.
pub fn strip_metadata(path: &str) -> StripMetadataResult
{
    let _gdiplus = match GdiPlusToken::startup()
    {
        Some(token) => token,
        None => return StripMetadataResult::Failed,
    };
    let image = match LoadedImage::load(path)
    {
        Some(image) => image,
        None => return StripMetadataResult::Failed,
    };

    let mut removed = false;
    for id in COMMON_TAGS
    {
        if gdiplus_helper::get_property(image.handle, id).is_none()
        {
            continue;
        }

        // SAFETY: `image.handle` is a live GDI+ image owned by `image`.
        if unsafe { GdipRemovePropertyItem(image.handle, id) } == 0
        {
            removed = true;
        }
    }

    if !removed
    {
        return StripMetadataResult::NoMetadataFound;
    }

    if gdiplus_helper::save_over(path, image)
    {
        StripMetadataResult::Stripped
    }
    else
    {
        StripMetadataResult::Failed
    }
}


/// Writes the supplied metadata fields onto the image at `path`, overwriting it
/// in place. Only `Some` fields are written; `None` fields are left as they are.
/// The image is re-encoded through GDI+, so persistence depends on the format's
/// encoder (JPEG and TIFF retain these tags; PNG and BMP support is limited).
/// Returns `true` on success.
pub fn write_metadata(path: &str, metadata: &Metadata) -> bool
{
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

    apply_ascii(image.handle, TAG_DOCUMENT_NAME, &metadata.document_name);
    apply_ascii(image.handle, TAG_IMAGE_DESCRIPTION, &metadata.description);
    apply_ascii(image.handle, TAG_SOFTWARE, &metadata.software);
    apply_ascii(image.handle, TAG_DATE_TIME, &metadata.date_time);
    apply_ascii(image.handle, TAG_ARTIST, &metadata.artist);
    apply_ascii(image.handle, TAG_HOST_COMPUTER, &metadata.host_computer);
    apply_ascii(image.handle, TAG_COPYRIGHT, &metadata.copyright);
    apply_xp_string(image.handle, TAG_XP_TITLE, &metadata.title);
    apply_xp_string(image.handle, TAG_XP_COMMENT, &metadata.comment);
    apply_xp_string(image.handle, TAG_XP_AUTHOR, &metadata.author);
    apply_xp_string(image.handle, TAG_XP_KEYWORDS, &metadata.keywords);
    apply_xp_string(image.handle, TAG_XP_SUBJECT, &metadata.subject);

    gdiplus_helper::save_over(path, image)
}


/// Writes an ASCII tag onto `image` when `value` is set and reports a failed tag.
fn apply_ascii(image: *mut GpImage, propid: u32, value: &Option<String>)
{
    let text = match value
    {
        Some(text) => text,
        None => return,
    };

    let mut bytes: Vec<u8> = text.bytes().collect();
    bytes.push(0);
    let _ = set_property(image, propid, TYPE_ASCII, &mut bytes);
}


/// Writes a UTF-16 XP tag onto `image` when `value` is set and reports a failed tag.
fn apply_xp_string(image: *mut GpImage, propid: u32, value: &Option<String>)
{
    let text = match value
    {
        Some(text) => text,
        None => return,
    };

    let mut bytes: Vec<u8> = Vec::with_capacity(text.len() * 2 + 2);
    for unit in text.encode_utf16()
    {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 0]);
    let _ = set_property(image, propid, TYPE_BYTE, &mut bytes);
}


/// Sets a single property item on `image` from a caller-owned value buffer.
/// Returns `true` when the item was set.
fn set_property(image: *mut GpImage, propid: u32, value_type: u16, bytes: &mut [u8]) -> bool
{
    let item = PropertyItem
    {
        id: propid,
        length: bytes.len() as u32,
        r#type: value_type,
        value: bytes.as_mut_ptr() as *mut c_void,
    };

    // SAFETY: `item.value` points into `bytes`, which outlives the call.
    let applied = unsafe { GdipSetPropertyItem(image, &item) == 0 };
    if !applied
    {
        eprintln!("metadata: failed to write property {propid:#06x}");
    }

    applied
}
