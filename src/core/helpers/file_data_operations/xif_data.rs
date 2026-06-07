//! Reading EXIF metadata points from photo files via GDI+.




use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::core::GUID;
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipDisposeImage, GdipGetImageEncoders, GdipGetImageEncodersSize, GdipGetImageRawFormat,
    GdipGetPropertyCount, GdipGetPropertyIdList, GdipGetPropertyItem, GdipGetPropertyItemSize,
    GdipLoadImageFromFile, GdipRemovePropertyItem, GdipSaveImageToFile, GdipSetPropertyItem,
    GdiplusShutdown, GdiplusStartup, GdiplusStartupInput, GpImage, ImageCodecInfo, PropertyItem,
};

use crate::core::helpers::file_operations::file_helper;

// EXIF tag identifiers as exposed by GDI+ property items.
const TAG_MAKE: u32 = 0x010F;
const TAG_MODEL: u32 = 0x0110;
const TAG_SOFTWARE: u32 = 0x0131;
const TAG_DATE_TIME_ORIGINAL: u32 = 0x9003;
const TAG_ORIENTATION: u32 = 0x0112;
const TAG_PIXEL_X_DIM: u32 = 0xA002;
const TAG_PIXEL_Y_DIM: u32 = 0xA003;
const TAG_GPS_LATITUDE_REF: u32 = 0x0001;
const TAG_GPS_LATITUDE: u32 = 0x0002;
const TAG_GPS_LONGITUDE_REF: u32 = 0x0003;
const TAG_GPS_LONGITUDE: u32 = 0x0004;
const TAG_GPS_ALTITUDE_REF: u32 = 0x0005;
const TAG_GPS_ALTITUDE: u32 = 0x0006;

// EXIF/GDI+ property value types.
const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;

/// The privacy-relevant EXIF points extracted from a photo. Every field is
/// optional because any individual tag may be absent from the image. GPS
/// coordinates are decimal degrees and altitude is metres, each already signed
/// by the matching reference tag (S/W and below-sea-level are negative).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExifData
{
    pub make: Option<String>,
    pub model: Option<String>,
    pub software: Option<String>,
    pub date_time_original: Option<String>,
    pub orientation: Option<u16>,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f64>,
}

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

/// Reads EXIF points from the photo at `path`. Returns `None` only when the
/// file cannot be opened as an image; a successfully opened image with no EXIF
/// yields an `ExifData` whose fields are all `None`.
pub fn read_exif(path: &str) -> Option<ExifData>
{
    let _gdiplus = GdiPlusToken::startup()?;
    let image = LoadedImage::load(path)?;
    let handle = image.handle;

    Some(ExifData
    {
        make: read_string(handle, TAG_MAKE),
        model: read_string(handle, TAG_MODEL),
        software: read_string(handle, TAG_SOFTWARE),
        date_time_original: read_string(handle, TAG_DATE_TIME_ORIGINAL),
        orientation: read_u16(handle, TAG_ORIENTATION),
        pixel_width: read_u32(handle, TAG_PIXEL_X_DIM),
        pixel_height: read_u32(handle, TAG_PIXEL_Y_DIM),
        gps_latitude: read_gps_coordinate(handle, TAG_GPS_LATITUDE, TAG_GPS_LATITUDE_REF, b'S'),
        gps_longitude: read_gps_coordinate(handle, TAG_GPS_LONGITUDE, TAG_GPS_LONGITUDE_REF, b'W'),
        gps_altitude: read_gps_altitude(handle),
    })
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
    else if file_helper::is_png(path)
    {
        strip_via_gdiplus(path)
    }
    else
    {
        strip_via_gdiplus(path)
    }
}


/// Writes the configured replacement value to text-based EXIF tags.
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

    save_image_over(path, image)
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


/// Fetches one property item, returning its EXIF value type and raw value bytes.
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


fn read_string(image: *mut GpImage, propid: u32) -> Option<String>
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

fn read_u16(image: *mut GpImage, propid: u32) -> Option<u16>
{
    let (value_type, bytes) = get_property(image, propid)?;

    if value_type != TYPE_SHORT || bytes.len() < 2
    {
        return None;
    }

    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(image: *mut GpImage, propid: u32) -> Option<u32>
{
    let (value_type, bytes) = get_property(image, propid)?;

    match value_type
    {
        TYPE_SHORT if bytes.len() >= 2 => Some(u16::from_le_bytes([bytes[0], bytes[1]]) as u32),
        TYPE_LONG if bytes.len() >= 4 => Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        _ => None,
    }
}

fn read_gps_coordinate(image: *mut GpImage, propid: u32, ref_propid: u32, negative_ref: u8) -> Option<f64>
{
    let (value_type, bytes) = get_property(image, propid)?;

    if value_type != TYPE_RATIONAL || bytes.len() < 24
    {
        return None;
    }

    let degrees = read_rational(&bytes[0..8])?;
    let minutes = read_rational(&bytes[8..16])?;
    let seconds = read_rational(&bytes[16..24])?;
    let mut coordinate = degrees + minutes / 60.0 + seconds / 3600.0;

    if let Some((ref_type, ref_bytes)) = get_property(image, ref_propid)
    {
        if ref_type == TYPE_ASCII && ref_bytes.first().copied() == Some(negative_ref)
        {
            coordinate = -coordinate;
        }
    }

    Some(coordinate)
}

fn read_gps_altitude(image: *mut GpImage) -> Option<f64>
{
    let (value_type, bytes) = get_property(image, TAG_GPS_ALTITUDE)?;
    if value_type != TYPE_RATIONAL || bytes.len() < 8
    {
        return None;
    }

    let mut altitude = read_rational(&bytes[0..8])?;

    if let Some((ref_type, ref_bytes)) = get_property(image, TAG_GPS_ALTITUDE_REF)
    {
        if ref_type == TYPE_BYTE && ref_bytes.first().copied() == Some(1)
        {
            altitude = -altitude;
        }
    }

    Some(altitude)
}

fn read_rational(bytes: &[u8]) -> Option<f64>
{
    if bytes.len() < 8
    {
        return None;
    }

    let numerator = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let denominator = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if denominator == 0
    {
        return None;
    }

    Some(numerator as f64 / denominator as f64)
}


/// Rewrites a JPEG without its EXIF APP1 segment(s), preserving every other
/// segment and the entropy-coded image data byte-for-byte (no recompression).
fn strip_jpeg_exif(path: &str) -> StripExifResult
{
    let data = match std::fs::read(path)
    {
        Ok(data) => data,
        Err(_) => return StripExifResult::Failed,
    };
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8
    {
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
            return StripExifResult::Failed;
        }

        let segment_start = pos;
        while pos < data.len() && data[pos] == 0xFF
        {
            pos += 1;
        }
        if pos >= data.len()
        {
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
            return StripExifResult::Failed;
        }
        let length = ((data[pos] as usize) << 8) | data[pos + 1] as usize;
        if length < 2 || pos + length > data.len()
        {
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

/// Commits stripped JPEG bytes to `path`, or reports that nothing was removed.
fn finish_jpeg(path: &str, output: &[u8], removed: bool) -> StripExifResult
{
    if !removed
    {
        return StripExifResult::NoExifFound;
    }

    let temp = temp_path(path);
    if std::fs::write(&temp, output).is_err()
    {
        let _ = std::fs::remove_file(&temp);
        return StripExifResult::Failed;
    }

    if commit_rename(&temp, path)
    {
        StripExifResult::Stripped
    }
    else
    {
        StripExifResult::Failed
    }
}

/// Removes every GDI+ property item from a non-JPEG image and re-encodes it.
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
        unsafe { GdipRemovePropertyItem(image.handle, *id) };
    }

    let mut clsid = GUID { data1: 0, data2: 0, data3: 0, data4: [0; 8] };
    
    if !find_encoder(image.handle, &mut clsid)
    {
        return StripExifResult::Failed;
    }

    let temp = temp_path(path);
    let wide: Vec<u16> = OsStr::new(&temp).encode_wide().chain(std::iter::once(0)).collect();

    let status = unsafe { GdipSaveImageToFile(image.handle, wide.as_ptr(), &clsid, ptr::null()) };
    if status != 0
    {
        let _ = std::fs::remove_file(&temp);
        return StripExifResult::Failed;
    }

    drop(image);

    if commit_rename(&temp, path)
    {
        StripExifResult::Stripped
    }
    else
    {
        StripExifResult::Failed
    }
}

/// Writes one ASCII property item, filtering bytes that are not valid ASCII.
fn set_ascii_property(image: *mut GpImage, propid: u32, value: &str) -> bool
{
    let mut bytes: Vec<u8> = value
        .bytes()
        .filter(|byte| byte.is_ascii() && !byte.is_ascii_control())
        .collect();

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

    unsafe { GdipSetPropertyItem(image, &item) == 0 }
}


/// Re-encodes `image` over `path` via a temp file and atomic rename.
fn save_image_over(path: &str, image: LoadedImage) -> bool
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


/// Returns the list of GDI+ property-item identifiers present on the image.
fn property_ids(image: *mut GpImage) -> Option<Vec<u32>>
{
    let mut count: u32 = 0;

    if unsafe { GdipGetPropertyCount(image, &mut count) } != 0
    {
        return None;
    }
    if count == 0
    {
        return Some(Vec::new());
    }

    let mut ids = vec![0u32; count as usize];

    if unsafe { GdipGetPropertyIdList(image, count, ids.as_mut_ptr()) } != 0
    {
        return None;
    }

    Some(ids)
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
