//! File-related helper routines.

use std::ptr;

use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&()-_=+[]{}";

/// Returns a random string between 8 and 14 characters long, drawn from
/// letters, digits, and filename-safe special characters.
///
/// Mainly going to use this for generating random file names for screenshots, maybe for randomization of xif / meta datas.
pub fn random_string() -> String
{
    let mut bytes = [0u8; 15];

    // SAFETY: `bytes` is a local buffer of exactly the length passed.
    unsafe { BCryptGenRandom(ptr::null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };

    let length = 8 + (bytes[0] as usize % 7);
    (0..length).map(|i| CHARSET[bytes[i + 1] as usize % CHARSET.len()] as char).collect()
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
