//! High-level workflow for saving captured images, cleaning metadata, applying
//! configured custom data, and reporting the result to the user.

use std::path::{Path, PathBuf};

use crate::core::base::configs::config_master::{self, Config};
use crate::core::base::notify::notifications_handler;
use crate::core::helpers::file_data_operations::metadata;
use crate::core::helpers::file_data_operations::xif_data;
use crate::core::helpers::file_operations::file_helper;
use crate::core::helpers::graphics::screen_capture::Screenshot;

use super::save_helpers::{copy_image_to_clipboard, encode_circular_image, encode_image, ImageFormat};

const IMAGES_DIR: &str = "Images";

/// Writes `screenshot` as a cleaned image, then applies the user's chosen
/// disposition: copy it to the clipboard, keep it on disk, or both. When
/// `circular` is set the image is masked to the circle inscribed in its
/// bounds (transparent outside for PNG, white for formats without alpha). The
/// file is always written first so the clipboard copy reads from the
/// metadata-stripped result; when auto-save is off it is removed once copied.
/// Returns the saved path when the file is kept, otherwise `None`.
pub fn save_screenshot(screenshot: &Screenshot, circular: bool) -> Option<PathBuf>
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

    let encoded = if circular { encode_circular_image(screenshot, &path_text, &format) } else { encode_image(screenshot.bitmap(), &path_text, format.encoder()) };

    if !encoded
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


/// Writes configured custom image data to the file at `path` when the user has
/// enabled replacements.
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

    if !exif_value.is_empty() && !xif_data::write_custom_exif(path, exif_value)
    {
        eprintln!("save: failed to write custom EXIF data");
    }

    let metadata_value = config.custom_data.metadata.trim();
    if !metadata_value.is_empty()
    {
        let metadata = metadata_from_value(metadata_value);
        if !metadata::write_metadata(path, &metadata)
        {
            eprintln!("save: failed to write custom metadata");
        }
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
    let mut directory = match std::env::current_dir()
    {
        Ok(directory) => directory,
        Err(_) =>
        {
            eprintln!("save: failed to resolve the working directory");
            return None;
        }
    };
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
