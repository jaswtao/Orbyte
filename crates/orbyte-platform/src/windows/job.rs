//! Windows Job Object 封装。
//!
//! 全局 Job：Orbyte 启动时把自身加入，崩溃或被结束时由系统清理全部后代。
//! 会话 Job：每个终端一个，用于单独结束该终端的进程树。
//!
//! Author: jaswtao
//! Date: 2026-10-09

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::ptr::{addr_of, null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// 进程 ID 列表查询的最大容量。
const MAX_LISTED_PROCESSES: usize = 4096;

/// 设置了 `KILL_ON_JOB_CLOSE` 的 Job Object，最后一个句柄关闭时结束其中全部进程。
///
/// 句柄以不可继承方式创建，避免子进程持有句柄导致 Job 无法关闭。
#[derive(Debug)]
pub struct JobObject {
    handle: HANDLE,
}

// Job 句柄是内核对象句柄，可在线程间安全传递和共享。
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}

impl JobObject {
    /// 创建一个关闭即结束全部进程的匿名 Job。
    pub fn new_kill_on_close() -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self { handle };

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// 把进程句柄加入本 Job。进程已在其他 Job 中时形成嵌套 Job（Windows 8 起支持）。
    pub fn assign_process(&self, process: *mut c_void) -> io::Result<()> {
        let ok = unsafe { AssignProcessToJobObject(self.handle, process as HANDLE) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// 把当前进程加入本 Job。
    pub fn assign_current_process(&self) -> io::Result<()> {
        self.assign_process(unsafe { GetCurrentProcess() })
    }

    /// 结束 Job 中的全部进程。
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        let ok = unsafe { TerminateJobObject(self.handle, exit_code) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// 列出 Job（含嵌套子 Job）中仍存活的进程 ID。
    pub fn process_ids(&self) -> io::Result<Vec<u32>> {
        let header = size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() - size_of::<usize>();
        let byte_len = header + MAX_LISTED_PROCESSES * size_of::<usize>();
        // 用 usize 数组保证对齐。
        let mut buffer = vec![0usize; byte_len.div_ceil(size_of::<usize>())];
        let list = buffer.as_mut_ptr() as *mut JOBOBJECT_BASIC_PROCESS_ID_LIST;
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle,
                JobObjectBasicProcessIdList,
                list as *mut c_void,
                byte_len as u32,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            let count = (*list).NumberOfProcessIdsInList as usize;
            let first = addr_of!((*list).ProcessIdList) as *const usize;
            Ok((0..count).map(|i| *first.add(i) as u32).collect())
        }
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

static GLOBAL_JOB: OnceLock<JobObject> = OnceLock::new();

/// 创建全局 Job 并把当前进程加入。之后创建的子进程默认继承该 Job。
///
/// 句柄在进程生命周期内不关闭：Orbyte 自身也在 Job 内，进程退出时由系统关闭句柄并清理残留进程。
pub fn install_global_job() -> io::Result<&'static JobObject> {
    if let Some(job) = GLOBAL_JOB.get() {
        return Ok(job);
    }
    let job = JobObject::new_kill_on_close()?;
    job.assign_current_process()?;
    Ok(GLOBAL_JOB.get_or_init(|| job))
}
