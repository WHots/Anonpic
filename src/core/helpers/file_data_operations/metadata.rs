//! Reading, removing, and writing common document/authoring metadata in image
//! files via GDI+. This complements `xif_data`, which focuses on camera and GPS
//! EXIF; here the focus is the identity-revealing authoring fields (artist,
//! copyright, software, host machine, and the Windows "Details" tab tags).




use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::core::GUID;
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipDisposeImage, GdipGetImageEncoders, GdipGetImageEncodersSize, GdipGetImageRawFormat,
    GdipGetPropertyItem, GdipGetPropertyItemSize, GdipLoadImageFromFile, GdipRemovePropertyItem,
    GdipSaveImageToFile, GdipSetPropertyItem, GdiplusShutdown, GdiplusStartup, GdiplusStartupInput,
    GpImage, ImageCodecInfo, PropertyItem,
};

use crate::core::helpers::file_operations::file_helper;

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

/// Reads the common metadata points from the image at `path`. Returns `None`
/// only when the file cannot be opened as an image; a successfully opened image
/// with no such metadata yields a `Metadata` whose fields are all `None`.
pub fn read_metadata(path: &str) -> Option<Metadata>
{
    let _gdiplus = GdiPlusToken::startup()?;
    let image = LoadedImage::load(path)?;
    let handle = image.handle;

    Some(Metadata
    {
        document_name: read_ascii(handle, TAG_DOCUMENT_NAME),
        description: read_ascii(handle, TAG_IMAGE_DESCRIPTION),
        software: read_ascii(handle, TAG_SOFTWARE),
        date_time: read_ascii(handle, TAG_DATE_TIME),
        artist: read_ascii(handle, TAG_ARTIST),
        host_computer: read_ascii(handle, TAG_HOST_COMPUTER),
        copyright: read_ascii(handle, TAG_COPYRIGHT),
        title: read_xp_string(handle, TAG_XP_TITLE),
        comment: read_xp_string(handle, TAG_XP_COMMENT),
        author: read_xp_string(handle, TAG_XP_AUTHOR),
        keywords: read_xp_string(handle, TAG_XP_KEYWORDS),
        subject: read_xp_string(handle, TAG_XP_SUBJECT),
    })
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
        if get_property(image.handle, id).is_none()
        {
            continue;
        }

        if unsafe { GdipRemovePropertyItem(image.handle, id) } == 0
        {
            removed = true;
        }
    }

    if !removed
    {
        return StripMetadataResult::NoMetadataFound;
    }

    if save_over(path, image)
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

    save_over(path, image)
}


/// RAII guard that initializes GDI+ on construction and shuts it down on drop.
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


/// RAII guard wrapping a loaded GDI+ image, disposed on drop.
struct LoadedImage
{
    handle: *mut GpImage,
}

impl LoadedImage
{
    fn load(path: &str) -> Option<Self>
    {
        let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
        let mut handle: *mut GpImage = ptr::null_mut();

        if unsafe { GdipLoadImageFromFile(wide.as_ptr(), &mut handle) } != 0 || handle.is_null()
        {
            return None;
        }

        Some(Self { handle })
    }
}

impl Drop for LoadedImage
{
    fn drop(&mut self)
    {
        unsafe { GdipDisposeImage(self.handle) };
    }
}


/// Fetches one property item, returning its value type and raw value bytes.
fn get_property(image: *mut GpImage, propid: u32) -> Option<(u16, Vec<u8>)>
{
    let mut size: u32 = 0;

    if unsafe { GdipGetPropertyItemSize(image, propid, &mut size) } != 0
    {
        return None;
    }
    if (size as usize) < std::mem::size_of::<PropertyItem>()
    {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];

    if unsafe { GdipGetPropertyItem(image, propid, size, buffer.as_mut_ptr() as *mut PropertyItem) } != 0
    {
        return None;
    }

    let item = unsafe { ptr::read_unaligned(buffer.as_ptr() as *const PropertyItem) };
    if item.value.is_null()
    {
        return None;
    }

    let value = unsafe { std::slice::from_raw_parts(item.value as *const u8, item.length as usize) };
    Some((item.r#type, value.to_vec()))
}

/// Reads a NUL-terminated ASCII tag, trimmed; `None` when absent or empty.
fn read_ascii(image: *mut GpImage, propid: u32) -> Option<String>
{
    let (value_type, bytes) = get_property(image, propid)?;

    if value_type != TYPE_ASCII
    {
        return None;
    }

    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).trim().to_string();

    if text.is_empty()
    {
        None
    }
    else
    {
        Some(text)
    }
}

/// Reads a NUL-terminated little-endian UTF-16 XP tag, trimmed; `None` when
/// absent or empty.
fn read_xp_string(image: *mut GpImage, propid: u32) -> Option<String>
{
    let (value_type, bytes) = get_property(image, propid)?;

    if value_type != TYPE_BYTE || bytes.len() < 2
    {
        return None;
    }

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    let text = String::from_utf16_lossy(&units).trim().to_string();

    if text.is_empty()
    {
        None
    }
    else
    {
        Some(text)
    }
}

/// Writes an ASCII tag when `value` is set, ignoring failures of a single tag.
fn apply_ascii(image: *mut GpImage, propid: u32, value: &Option<String>)
{
    let text = match value
    {
        Some(text) => text,
        None => return,
    };

    let mut bytes: Vec<u8> = text.bytes().collect();
    bytes.push(0);
    set_property(image, propid, TYPE_ASCII, &mut bytes);
}

/// Writes a UTF-16 XP tag when `value` is set, ignoring failures of a single tag.
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
    set_property(image, propid, TYPE_BYTE, &mut bytes);
}

/// Sets a single property item from a caller-owned value buffer.
fn set_property(image: *mut GpImage, propid: u32, value_type: u16, bytes: &mut [u8]) -> bool
{
    let item = PropertyItem
    {
        id: propid,
        length: bytes.len() as u32,
        r#type: value_type,
        value: bytes.as_mut_ptr() as *mut c_void,
    };

    unsafe { GdipSetPropertyItem(image, &item) == 0 }
}

/// Re-encodes `image` over `path` via a temp file and atomic rename, choosing the
/// encoder that matches the image's own format. Consumes `image` so its lock on
/// the original file is released before the rename.
fn save_over(path: &str, image: LoadedImage) -> bool
{
    let mut clsid = GUID { data1: 0, data2: 0, data3: 0, data4: [0; 8] };

    if !find_encoder(image.handle, &mut clsid)
    {
        return false;
    }

    let temp = temp_path(path);
    let wide: Vec<u16> = OsStr::new(&temp).encode_wide().chain(std::iter::once(0)).collect();

    let status = unsafe { GdipSaveImageToFile(image.handle, wide.as_ptr(), &clsid, ptr::null()) };
    if status != 0
    {
        let _ = std::fs::remove_file(&temp);
        return false;
    }

    drop(image);

    commit_rename(&temp, path)
}

/// Finds the encoder CLSID matching the image's raw format, writing it to `clsid`.
fn find_encoder(image: *mut GpImage, clsid: &mut GUID) -> bool
{
    let mut format = GUID { data1: 0, data2: 0, data3: 0, data4: [0; 8] };

    if unsafe { GdipGetImageRawFormat(image, &mut format) } != 0
    {
        return false;
    }

    let mut num: u32 = 0;
    let mut size: u32 = 0;

    if unsafe { GdipGetImageEncodersSize(&mut num, &mut size) } != 0 || num == 0 || size == 0
    {
        return false;
    }

    let mut buffer = vec![0u8; size as usize];

    if unsafe { GdipGetImageEncoders(num, size, buffer.as_mut_ptr() as *mut ImageCodecInfo) } != 0
    {
        return false;
    }

    let stride = std::mem::size_of::<ImageCodecInfo>();
    for i in 0..num as usize
    {
        let codec = unsafe { ptr::read_unaligned(buffer.as_ptr().add(i * stride) as *const ImageCodecInfo) };
        if guid_eq(&codec.FormatID, &format)
        {
            *clsid = codec.Clsid;
            return true;
        }
    }

    false
}

/// Compares two GUIDs field by field (windows-sys `GUID` is not `PartialEq`).
fn guid_eq(a: &GUID, b: &GUID) -> bool
{
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// Builds a unique sibling temp path next to `path`, on the same volume.
fn temp_path(path: &str) -> String
{
    format!("{path}.{}.tmp", file_helper::random_string())
}

/// Renames `temp` onto `path`, deleting `temp` if the replace fails.
fn commit_rename(temp: &str, path: &str) -> bool
{
    if std::fs::rename(temp, path).is_ok()
    {
        true
    }
    else
    {
        let _ = std::fs::remove_file(temp);
        false
    }
}
