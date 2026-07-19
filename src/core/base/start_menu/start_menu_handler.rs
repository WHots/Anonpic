//! Start Menu integration: creating and removing the per-user Start Menu
//! shortcut (`.lnk`) so Anonpic shows up in Windows search. The shortcut's
//! SHA-256 hash is recorded at creation and verified before removal, so a
//! foreign file that happens to share the name is never deleted.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::core::GUID;
use windows_sys::Win32::Security::Cryptography::{BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCryptOpenAlgorithmProvider, BCRYPT_SHA256_ALGORITHM};
use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};

use crate::core::base::configs::config_master;
use crate::core::helpers::graphics::gdiplus_helper::wide;

const SHORTCUT_NAME: &str = "Anonpic.lnk";
const HASH_FILE: &str = "start_menu.hash";

/// CLSID of the shell's ShellLink coclass.
const CLSID_SHELL_LINK: GUID = GUID
{
    data1: 0x0002_1401,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

/// IID of `IShellLinkW`.
const IID_ISHELL_LINK_W: GUID = GUID
{
    data1: 0x0002_14F9,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

/// IID of `IPersistFile`.
const IID_IPERSIST_FILE: GUID = GUID
{
    data1: 0x0000_010B,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

/// Raw COM object layout: a single pointer to the interface's vtable.
#[repr(C)]
struct ComObject<T>
{
    vtbl: *const T,
}

/// Manual `IShellLinkW` vtable. Only the methods this module calls carry real
/// signatures; the rest are pointer-sized placeholders that keep the layout
/// identical to the shell's vtable.
#[repr(C)]
struct IShellLinkWVtbl
{
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_path: usize,
    get_id_list: usize,
    set_id_list: usize,
    get_description: usize,
    set_description: unsafe extern "system" fn(*mut c_void, *const u16) -> i32,
    get_working_directory: usize,
    set_working_directory: unsafe extern "system" fn(*mut c_void, *const u16) -> i32,
    get_arguments: usize,
    set_arguments: usize,
    get_hotkey: usize,
    set_hotkey: usize,
    get_show_cmd: usize,
    set_show_cmd: usize,
    get_icon_location: usize,
    set_icon_location: usize,
    set_relative_path: usize,
    resolve: usize,
    set_path: unsafe extern "system" fn(*mut c_void, *const u16) -> i32,
}

/// Manual `IPersistFile` vtable, laid out like [`IShellLinkWVtbl`].
#[repr(C)]
struct IPersistFileVtbl
{
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_class_id: usize,
    is_dirty: usize,
    load: usize,
    save: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32,
    save_completed: usize,
    get_cur_file: usize,
}


/// Applies the Start Menu setting: ensures the shortcut exists when `enabled`,
/// removes it (after hash verification) when disabled.
pub fn apply(enabled: bool)
{
    if enabled
    {
        create_shortcut();
    }
    else
    {
        remove_shortcut();
    }
}


/// Loads the saved Start Menu setting (enabled by default when no config has
/// been written yet) and applies it. Called once at startup.
pub fn apply_saved()
{
    let enabled = config_master::load_config().map(|config| config.start_menu_shortcut).unwrap_or(true);

    apply(enabled);
}


/// Creates the per-user Start Menu shortcut pointing at the running executable
/// and records its SHA-256 hash for later verified removal. Does nothing but
/// backfill the hash when the shortcut already exists.
fn create_shortcut()
{
    let path = match shortcut_path()
    {
        Some(path) => path,
        None =>
        {
            eprintln!("start_menu: APPDATA not set; cannot locate Start Menu");
            return;
        }
    };

    if path.exists()
    {
        ensure_hash_recorded(&path);
        return;
    }

    let target = match std::env::current_exe()
    {
        Ok(target) => target,
        Err(_) =>
        {
            eprintln!("start_menu: failed to resolve the executable path");
            return;
        }
    };

    if !write_shortcut(&path, &target)
    {
        eprintln!("start_menu: failed to create shortcut: {}", path.display());
        return;
    }

    record_hash(&path);
}


/// Removes the Start Menu shortcut when it exists and its SHA-256 hash matches
/// the hash recorded at creation; a missing shortcut just clears the stored
/// hash, and a mismatched one is left untouched.
fn remove_shortcut()
{
    let path = match shortcut_path()
    {
        Some(path) => path,
        None => return,
    };

    if !path.exists()
    {
        if let Some(hash_file) = hash_path()
        {
            let _ = std::fs::remove_file(hash_file);
        }
        return;
    }

    let stored = hash_path().and_then(|file| std::fs::read_to_string(file).ok()).map(|text| text.trim().to_string());
    let actual = sha256_file(&path);

    match (stored, actual)
    {
        (Some(stored), Some(actual)) if stored == actual =>
        {
            let _ = std::fs::remove_file(&path);

            if let Some(hash_file) = hash_path()
            {
                let _ = std::fs::remove_file(hash_file);
            }
        }
        _ => eprintln!("start_menu: shortcut hash missing or mismatched; leaving {} in place", path.display()),
    }
}


/// Writes a `.lnk` at `link_path` pointing at `target` through the shell's
/// `IShellLinkW`/`IPersistFile` COM objects. Returns `true` on success.
fn write_shortcut(link_path: &Path, target: &Path) -> bool
{
    let target_wide = wide(&target.to_string_lossy());
    let working_dir_wide = target.parent().map(|dir| wide(&dir.to_string_lossy()));
    let link_wide = wide(&link_path.to_string_lossy());
    let description = wide("Anonpic");

    // SAFETY: COM is initialized for the duration of the call, every acquired
    // interface is released before returning, and all pointers reference
    // locals that outlive the calls.
    unsafe
    {
        let com = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32);
        if com < 0
        {
            eprintln!("start_menu: failed to initialize COM");
            return false;
        }

        let mut link: *mut ComObject<IShellLinkWVtbl> = ptr::null_mut();
        let created = CoCreateInstance(&CLSID_SHELL_LINK, ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ISHELL_LINK_W, &mut link as *mut _ as *mut *mut c_void);
        if created < 0 || link.is_null()
        {
            eprintln!("start_menu: failed to create ShellLink object");
            CoUninitialize();
            return false;
        }

        let vtbl = &*(*link).vtbl;
        (vtbl.set_path)(link as *mut c_void, target_wide.as_ptr());
        (vtbl.set_description)(link as *mut c_void, description.as_ptr());

        if let Some(dir) = &working_dir_wide
        {
            (vtbl.set_working_directory)(link as *mut c_void, dir.as_ptr());
        }

        let mut persist: *mut ComObject<IPersistFileVtbl> = ptr::null_mut();
        let queried = (vtbl.query_interface)(link as *mut c_void, &IID_IPERSIST_FILE, &mut persist as *mut _ as *mut *mut c_void);

        let mut saved = false;
        if queried >= 0 && !persist.is_null()
        {
            let persist_vtbl = &*(*persist).vtbl;
            saved = (persist_vtbl.save)(persist as *mut c_void, link_wide.as_ptr(), 1) >= 0;
            (persist_vtbl.release)(persist as *mut c_void);
        }

        (vtbl.release)(link as *mut c_void);
        CoUninitialize();

        saved
    }
}


/// Records the shortcut's SHA-256 hash next to the app config for later
/// verified removal.
fn record_hash(shortcut: &Path)
{
    let hash = match sha256_file(shortcut)
    {
        Some(hash) => hash,
        None => return,
    };

    let hash_file = match hash_path()
    {
        Some(hash_file) => hash_file,
        None => return,
    };

    if let Some(dir) = hash_file.parent()
    {
        let _ = std::fs::create_dir_all(dir);
    }

    if std::fs::write(&hash_file, hash).is_err()
    {
        eprintln!("start_menu: failed to record shortcut hash");
    }
}


/// Backfills the stored hash when the shortcut exists but no hash was
/// recorded, e.g. after an upgrade from a build without hash tracking.
fn ensure_hash_recorded(shortcut: &Path)
{
    let missing = hash_path().map(|file| !file.exists()).unwrap_or(false);

    if missing
    {
        record_hash(shortcut);
    }
}


/// Computes the SHA-256 of the file at `path` as lowercase hex, or `None` when
/// the file cannot be read or hashing fails.
fn sha256_file(path: &Path) -> Option<String>
{
    let data = match std::fs::read(path)
    {
        Ok(data) => data,
        Err(_) =>
        {
            eprintln!("start_menu: failed to read {}", path.display());
            return None;
        }
    };

    sha256_hex(&data)
}


/// Hashes `data` with CNG's SHA-256 provider, returning lowercase hex.
fn sha256_hex(data: &[u8]) -> Option<String>
{
    let mut digest = [0u8; 32];

    // SAFETY: all out-parameters are locals, `data` outlives the hashing call,
    // and every opened handle is closed before returning.
    unsafe
    {
        let mut algorithm: *mut c_void = ptr::null_mut();
        if BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0) != 0
        {
            eprintln!("start_menu: failed to open SHA-256 provider");
            return None;
        }

        let mut hash: *mut c_void = ptr::null_mut();
        if BCryptCreateHash(algorithm, &mut hash, ptr::null_mut(), 0, ptr::null(), 0, 0) != 0
        {
            eprintln!("start_menu: failed to create SHA-256 hash");
            BCryptCloseAlgorithmProvider(algorithm, 0);
            return None;
        }

        let hashed = BCryptHashData(hash, data.as_ptr(), data.len() as u32, 0) == 0 && BCryptFinishHash(hash, digest.as_mut_ptr(), digest.len() as u32, 0) == 0;

        BCryptDestroyHash(hash);
        BCryptCloseAlgorithmProvider(algorithm, 0);

        if !hashed
        {
            eprintln!("start_menu: failed to compute SHA-256");
            return None;
        }
    }

    Some(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}


/// Returns the per-user Start Menu shortcut path
/// (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Anonpic.lnk`), or `None`
/// when `APPDATA` is not set.
fn shortcut_path() -> Option<PathBuf>
{
    let appdata = std::env::var_os("APPDATA")?;

    Some(Path::new(&appdata).join("Microsoft").join("Windows").join("Start Menu").join("Programs").join(SHORTCUT_NAME))
}


/// Returns the path of the stored shortcut hash inside the app's config
/// directory, or `None` when the working directory cannot be determined.
fn hash_path() -> Option<PathBuf>
{
    Some(config_master::config_dir()?.join(HASH_FILE))
}
