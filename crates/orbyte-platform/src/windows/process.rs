//! Windows 进程查询辅助函数。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

const STILL_ACTIVE: u32 = 259;

/// Query the console host without attaching the application to another console.
/// Unsupported queries, inaccessible processes and detached processes return None.
pub fn process_console_host(pid: u32) -> Option<usize> {
    use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;

    // ProcessConsoleHostProcess (49) returns an ULONG_PTR. This native information
    // class is also named ProcessOwnerInformation in the Windows bindings.
    const PROCESS_CONSOLE_HOST_PROCESS: i32 = 49;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return None; }
        let mut host: usize = 0;
        let status = NtQueryInformationProcess(handle, PROCESS_CONSOLE_HOST_PROCESS,
            (&mut host as *mut usize).cast(), std::mem::size_of::<usize>() as u32,
            std::ptr::null_mut());
        CloseHandle(handle);
        // The low two bits are flags rather than part of the process ID.
        host &= !3;
        (status >= 0 && host != 0).then_some(host)
    }
}

/// 查询可执行文件路径；不读取或暴露进程命令行参数。
pub fn process_image(pid: u32) -> std::io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return Err(std::io::Error::last_os_error()); }
        let mut buffer = vec![0u16; 32768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
        let result = if ok == 0 { Err(std::io::Error::last_os_error()) }
            else { Ok(std::ffi::OsString::from_wide(&buffer[..length as usize]).into()) };
        CloseHandle(handle);
        result
    }
}

/// 判断指定 PID 的进程是否仍在运行。
pub fn is_process_alive(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}
