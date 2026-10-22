//! Suppress bare Alt, F10, and Alt+Space activation of the native window menu.
//! Author: jaswtao · Date: 2026-10-22

use std::io;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{SC_KEYMENU, WM_NCDESTROY, WM_SYSCOMMAND};

const SUBCLASS_ID: usize = 1;

/// Suppress bare Alt, F10, and Alt+Space menu activation
/// without consuming application keyboard events.
///
/// Other Alt shortcuts pass through.
/// `SC_KEYMENU` is swallowed when `lParam` is 0 or the space character.
///
/// # Safety
/// `hwnd` must be a live window owned by the calling thread.
pub unsafe fn disable_keyboard_menu(hwnd: HWND) -> io::Result<()> {
    if unsafe { SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID, 0) } == 0 {
        return Err(io::Error::other("Failed to disable keyboard activation of the window menu"));
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    // Windows reserves the low four bits of system commands for internal use.
    // Bare Alt and F10 use lParam 0. Alt+Space carries the space character.
    // Any other character is an application shortcut and stays forwarded.
    if message == WM_SYSCOMMAND
        && (wparam & 0xfff0) == SC_KEYMENU as usize
        && (lparam == 0 || lparam == ' ' as LPARAM)
    {
        return 0;
    }
    if message == WM_NCDESTROY {
        unsafe { RemoveWindowSubclass(hwnd, Some(window_proc), subclass_id) };
    }
    // SAFETY: Windows invokes this callback on the owning thread with valid message arguments.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

