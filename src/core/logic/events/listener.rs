//! Listens for key-press events. Hard-coded to the Print Screen key for now.

use std::ptr;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_SNAPSHOT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};

// The key we listen for. Hard-coded to Print Screen for now.
const TARGET_VK: u32 = VK_SNAPSHOT as u32;

/// Installs a global low-level keyboard hook and pumps the thread's message
/// queue until `WM_QUIT`. Blocks the calling thread, so run it on its own.
pub fn listen()
{
    
    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };

    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hinstance, 0) };
    if hook.is_null()
    {
        eprintln!("failed to install keyboard hook");
        return;
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0
    {
    }

    unsafe { UnhookWindowsHookEx(hook) };
}


unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT
{
    let is_key_down = wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN;
    if code == HC_ACTION as i32 && is_key_down
    {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        if kb.vkCode == TARGET_VK
        {
            on_print_screen();
        }
    }

    CallNextHookEx(ptr::null_mut(), code, wparam, lparam)
}

fn on_print_screen()
{
    crate::core::base::screen_grab::free_roam_screen_grab::spawn_capture();
}
