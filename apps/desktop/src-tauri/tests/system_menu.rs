//! Native window menu regression coverage.
//! Author: jaswtao · Date: 2026-10-22
#![cfg(windows)]

mod tests {
    use orbyte_platform::windows::system_menu::disable_keyboard_menu;
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, SendMessageW, SetWindowLongPtrW,
        GWLP_WNDPROC, SC_CLOSE, SC_KEYMENU, SC_MAXIMIZE, SC_MINIMIZE, SC_MOUSEMENU, SC_RESTORE,
        WM_APP, WM_NCDESTROY, WM_SYSCOMMAND,
    };

    const FORWARDED: LRESULT = 42;

    /// Result of the keyboard-menu command sent while `WM_NCDESTROY` is still on the stack.
    static NCDESTROY_KEYMENU: AtomicIsize = AtomicIsize::new(-1);

    unsafe extern "system" fn keep_window_alive(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCDESTROY {
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    unsafe extern "system" fn observe_messages(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        _reference_data: usize,
    ) -> LRESULT {
        // The menu subclass sits above this one and removes itself before forwarding
        // WM_NCDESTROY. A nested command shows whether that removal already happened.
        if message == WM_NCDESTROY {
            let result = unsafe { SendMessageW(hwnd, WM_SYSCOMMAND, SC_KEYMENU as usize, 0) };
            NCDESTROY_KEYMENU.store(result, Ordering::Relaxed);
        }
        if message == WM_SYSCOMMAND || message == WM_APP {
            return FORWARDED;
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    #[test]
    fn blocks_keyboard_menu_and_preserves_other_window_messages() {
        // A hidden native window exercises the real subclass chain without opening a menu.
        unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(
                0, class.as_ptr(), null(), 0, 0, 0, 100, 100,
                null_mut(), null_mut(), null_mut(), null(),
            );
            assert!(!hwnd.is_null());
            let procedure = keep_window_alive as *const () as usize as isize;
            assert_ne!(SetWindowLongPtrW(hwnd, GWLP_WNDPROC, procedure), 0);
            assert_ne!(SetWindowSubclass(hwnd, Some(observe_messages), 1, 0), 0);
            assert_eq!(
                SendMessageW(hwnd, WM_SYSCOMMAND, SC_KEYMENU as usize, 0),
                FORWARDED,
            );

            disable_keyboard_menu(hwnd).unwrap();
            disable_keyboard_menu(hwnd).unwrap();
            for command in [SC_KEYMENU, SC_KEYMENU | 0x0f] {
                for key in [0, ' ' as LPARAM] {
                    assert_eq!(SendMessageW(hwnd, WM_SYSCOMMAND, command as usize, key), 0);
                }
                assert_eq!(
                    SendMessageW(hwnd, WM_SYSCOMMAND, command as usize, '1' as LPARAM),
                    FORWARDED,
                );
            }
            for command in [SC_CLOSE, SC_MINIMIZE, SC_MAXIMIZE, SC_RESTORE, SC_MOUSEMENU] {
                assert_eq!(SendMessageW(hwnd, WM_SYSCOMMAND, command as usize, 0), FORWARDED);
            }
            assert_eq!(SendMessageW(hwnd, WM_APP, 0, 0), FORWARDED);

            NCDESTROY_KEYMENU.store(-1, Ordering::Relaxed);
            assert_eq!(SendMessageW(hwnd, WM_NCDESTROY, 0, 0), 0);
            assert_eq!(NCDESTROY_KEYMENU.load(Ordering::Relaxed), FORWARDED);
            assert_ne!(DestroyWindow(hwnd), 0);
        }
    }
}
