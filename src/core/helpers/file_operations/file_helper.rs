//! File-related helper routines.




use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&()-_=+[]{}";

/// Outcome of [`create_file`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CreateFileResult
{
    /// The directory or file could not be created.
    Failed = 0,
    /// The file already existed and was overwritten.
    Overwritten = 1,
    /// The file did not exist and was created new.
    CreatedNew = 2,
    /// The file existed and `overwrite` was false, so it was left untouched.
    NotOverwritten = 3,
}




/// Returns a random string between 8 and 14 characters long, drawn from
/// letters, digits, and filename-safe special characters.
/// 
/// Mainly going to use this for generating random file names for screenshots, maybe for randomization of xif / meta datas.
pub fn random_string() -> String
{
    let mut bytes = [0u8; 15];
    unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };

    let length = 8 + (bytes[0] as usize % 7);
    (0..length)
        .map(|i| CHARSET[bytes[i + 1] as usize % CHARSET.len()] as char)
        .collect()
}


/// Ensures the directory at `path` exists, creating it (and any missing parent
/// directories) if it is not already present. Returns `true` if the directory
/// exists after the call.
pub fn create_directory(path: &str) -> bool
{
    if does_dir_exist(path)
    {
        return true;
    }

    std::fs::create_dir_all(path).is_ok()
}


/// Creates `file_name` inside `directory`, ensuring the directory exists first.
/// When the file already exists it is overwritten only if `overwrite` is true.
pub fn create_file(directory: &str, file_name: &str, overwrite: bool) -> CreateFileResult
{
    if !create_directory(directory)
    {
        return CreateFileResult::Failed;
    }

    let path = Path::new(directory).join(file_name);

    // Create atomically so the existence check and the create cannot race:
    // `create_new` fails with `AlreadyExists` rather than silently clobbering
    // a file (or a symlink planted after a separate check) at the same path.
    match OpenOptions::new().write(true).create_new(true).open(&path)
    {
        Ok(_) => CreateFileResult::CreatedNew,
        Err(e) if e.kind() == ErrorKind::AlreadyExists =>
        {
            if !overwrite
            {
                return CreateFileResult::NotOverwritten;
            }

            match OpenOptions::new().write(true).create(true).truncate(true).open(&path)
            {
                Ok(_) => CreateFileResult::Overwritten,
                Err(_) => CreateFileResult::Failed,
            }
        }
        Err(_) => CreateFileResult::Failed,
    }
}


/// Returns `true` only if `path` exists and is a regular file. The path is not
/// followed through symlinks or reparse points, so a planted link does not pass
/// as a file.
pub fn does_file_exist(path: &str) -> bool
{
    match std::fs::symlink_metadata(path)
    {
        Ok(meta) => meta.is_file() && !meta.file_type().is_symlink(),
        Err(_) => false,
    }
}


/// Returns `true` if `path` begins with the JPEG signature.
pub fn is_jpeg(path: &str) -> bool
{
    use std::io::Read;

    let mut header = [0u8; 3];
    match std::fs::File::open(path)
    {
        Ok(mut file) => file.read_exact(&mut header).is_ok() && header == [0xFF, 0xD8, 0xFF],
        Err(_) => false,
    }
}


/// Returns `true` only if `path` exists and is a real directory. Directory
/// symlinks and junctions are rejected so writes cannot be redirected through a
/// planted reparse point.
fn does_dir_exist(path: &str) -> bool
{
    match std::fs::symlink_metadata(path)
    {
        Ok(meta) => meta.is_dir() && !meta.file_type().is_symlink(),
        Err(_) => false,
    }
}
