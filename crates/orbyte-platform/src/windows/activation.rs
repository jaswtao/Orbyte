//! 只在宿主保持前台交互时激活当前终端的新窗口。
//! Author: jaswtao · Date: 2026-10-16

use std::collections::HashSet;
use std::io;
use std::ptr::null_mut;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::Threading::AttachThreadInput;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_EVENT: u32 = WM_APP + 17;
const RETRIES_MS: [u64; 6] = [0, 50, 150, 300, 600, 1000];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Window {
    hwnd: usize,
    pid: u32,
}

struct Pending {
    window: Window,
    started: Instant,
    attempt: usize,
}

#[derive(Default)]
struct ActivationPolicy {
    seen: HashSet<(usize, u32)>,
    pending: Option<Pending>,
}

impl ActivationPolicy {
    fn shown(&mut self, window: Window, eligible: bool, now: Instant) {
        if self.seen.insert((window.hwnd, window.pid)) && eligible {
            self.pending = Some(Pending {
                window,
                started: now,
                attempt: 0,
            });
        }
    }

    fn destroyed(&mut self, hwnd: usize) {
        self.seen.retain(|&(value, _)| value != hwnd);
        if self.pending.as_ref().is_some_and(|p| p.window.hwnd == hwnd) {
            self.pending = None;
        }
    }

    fn foreground(&mut self, related: bool) {
        if !related {
            self.pending = None;
        }
    }

    fn due(&mut self, now: Instant) -> Option<Window> {
        let pending = self.pending.as_mut()?;
        let delay = RETRIES_MS.get(pending.attempt)?;
        if now.duration_since(pending.started) < Duration::from_millis(*delay) {
            return None;
        }
        pending.attempt += 1;
        Some(pending.window)
    }

    fn exhausted(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p.attempt == RETRIES_MS.len())
    }
}

/// 自有消息线程接收 WinEvent；前端只能选择会话，不能指定目标窗口。
pub struct WindowActivation {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl WindowActivation {
    pub fn start(
        main_hwnd: usize,
        is_active_process: impl Fn(u32) -> bool + Send + 'static,
    ) -> io::Result<Self> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("terminal-window-activation".into())
            .spawn(move || {
                let hooks = match Hooks::install() {
                    Ok(hooks) => hooks,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                let thread_id = unsafe { GetCurrentThreadId() };
                if ready_tx.send(Ok(thread_id)).is_err() {
                    return;
                }
                run(main_hwnd, &is_active_process);
                drop(hooks);
            })?;
        match ready_rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(io::Error::other(error))
            }
        }
    }
}

impl Drop for WindowActivation {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Hooks(Vec<HWINEVENTHOOK>);

impl Hooks {
    fn install() -> io::Result<Self> {
        // PostThreadMessage 要求目标消息队列已创建。
        let mut message = unsafe { std::mem::zeroed() };
        unsafe {
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
        }
        let mut hooks = Self(Vec::new());
        for (first, last) in [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_OBJECT_DESTROY, EVENT_OBJECT_SHOW),
        ] {
            let hook = unsafe {
                SetWinEventHook(
                    first,
                    last,
                    null_mut(),
                    Some(window_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_null() {
                return Err(io::Error::last_os_error());
            }
            hooks.0.push(hook);
        }
        Ok(hooks)
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        for hook in &self.0 {
            unsafe {
                UnhookWinEvent(*hook);
            }
        }
    }
}

unsafe extern "system" fn window_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if hwnd.is_null()
        || (event != EVENT_SYSTEM_FOREGROUND
            && (object != OBJID_WINDOW || child != CHILDID_SELF as i32))
    {
        return;
    }
    // OUTOFCONTEXT 回调在注册钩子的线程执行；消息循环负责查询进程与激活。
    unsafe {
        PostThreadMessageW(
            GetCurrentThreadId(),
            WINDOW_EVENT,
            event as WPARAM,
            hwnd as LPARAM,
        );
    }
}

fn window(hwnd: HWND) -> Window {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    Window {
        hwnd: hwnd as usize,
        pid,
    }
}

fn candidate(hwnd: HWND) -> bool {
    unsafe {
        IsWindow(hwnd) != 0
            && GetAncestor(hwnd, GA_ROOT) == hwnd
            && (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE))
                == 0
    }
}

fn interactive(hwnd: HWND) -> bool {
    candidate(hwnd)
        && unsafe {
            IsWindowVisible(hwnd) != 0
                && IsIconic(hwnd) == 0
                && GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_DISABLED == 0
        }
}

struct InputAttachment {
    from: u32,
    to: u32,
    attached: bool,
}

impl InputAttachment {
    fn new(from: u32, to: u32) -> Self {
        Self {
            from,
            to,
            attached: to != 0 && to != from && unsafe { AttachThreadInput(from, to, 1) } != 0,
        }
    }
}

impl Drop for InputAttachment {
    fn drop(&mut self) {
        if self.attached {
            unsafe {
                AttachThreadInput(self.from, self.to, 0);
            }
        }
    }
}

/// 在有独立消息队列的监听线程执行；只临时连接本宿主的 WebView2 输入线程。
fn activate_if_foreground(main: usize, target: usize, pid: u32) -> bool {
    let hwnd = target as HWND;
    if window(hwnd).pid != pid
        || !interactive(hwnd)
        || unsafe { GetForegroundWindow() } as usize != main
    {
        return false;
    }
    // WebView2 的键盘焦点属于渲染进程；临时共享其输入队列才能转交前台资格。
    // 只连接本宿主的焦点线程和已验证的终端子窗口，不连接无关应用。
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    let current_thread = unsafe { GetCurrentThreadId() };
    let focus_thread = if unsafe { GetGUIThreadInfo(0, &mut info) } != 0
        && !info.hwndFocus.is_null()
        && unsafe { GetAncestor(info.hwndFocus, GA_ROOT) } as usize == main
    {
        unsafe { GetWindowThreadProcessId(info.hwndFocus, null_mut()) }
    } else {
        0
    };
    let _focus_input = InputAttachment::new(current_thread, focus_thread);
    if unsafe { GetForegroundWindow() } as usize != main {
        return false;
    }
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, null_mut()) };
    let _target_input = InputAttachment::new(
        current_thread,
        if target_thread == focus_thread {
            0
        } else {
            target_thread
        },
    );
    // 共享队列可能已完成激活；否则只在宿主仍位于前台时发起切换。
    let before = unsafe { GetForegroundWindow() } as usize;
    before == target || (before == main && unsafe { SetForegroundWindow(hwnd) } != 0)
}

fn run(main: usize, is_active_process: &impl Fn(u32) -> bool) {
    let mut policy = ActivationPolicy::default();
    let mut timer = 0;
    let mut message = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        if message.message == WINDOW_EVENT {
            let hwnd = message.lParam as HWND;
            match message.wParam as u32 {
                EVENT_OBJECT_DESTROY => policy.destroyed(hwnd as usize),
                EVENT_SYSTEM_FOREGROUND => {
                    policy.foreground(hwnd as usize == main || is_active_process(window(hwnd).pid));
                }
                EVENT_OBJECT_SHOW if candidate(hwnd) => {
                    let target = window(hwnd);
                    let foreground = window(unsafe { GetForegroundWindow() });
                    let eligible = is_active_process(target.pid)
                        && (foreground.hwnd == main || is_active_process(foreground.pid));
                    log::debug!(
                        "终端窗口显示 hwnd={} pid={} eligible={eligible}",
                        target.hwnd,
                        target.pid
                    );
                    policy.shown(target, eligible, Instant::now());
                }
                _ => {}
            }
        }

        if let Some(target) = policy.due(Instant::now()) {
            let hwnd = target.hwnd as HWND;
            let foreground = window(unsafe { GetForegroundWindow() });
            if window(hwnd) != target
                || !candidate(hwnd)
                || unsafe { IsIconic(hwnd) } != 0
                || !is_active_process(target.pid)
                || foreground == target
            {
                policy.pending = None;
            } else if foreground.hwnd == main {
                let accepted =
                    interactive(hwnd) && activate_if_foreground(main, target.hwnd, target.pid);
                let focused = unsafe { GetForegroundWindow() } == hwnd;
                log::debug!(
                    "终端窗口激活 hwnd={} pid={} accepted={accepted} focused={focused}",
                    target.hwnd,
                    target.pid
                );
                if focused {
                    policy.pending = None;
                }
            } else if foreground.hwnd != 0 && !is_active_process(foreground.pid) {
                policy.pending = None;
            }
        }
        if policy.exhausted() {
            log::debug!("终端窗口激活重试结束");
            policy.pending = None;
        }
        // 空闲时没有轮询；只有窗口等待激活的最多一秒内才启动定时器。
        if policy.pending.is_some() && timer == 0 {
            timer = unsafe { SetTimer(null_mut(), 0, 25, None) };
            if timer == 0 {
                policy.pending = None;
                log::warn!("无法创建窗口激活重试定时器");
            }
        } else if policy.pending.is_none() && timer != 0 {
            unsafe {
                KillTimer(null_mut(), timer);
            }
            timer = 0;
        }
    }
    if timer != 0 {
        unsafe {
            KillTimer(null_mut(), timer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TARGET: Window = Window { hwnd: 1, pid: 2 };

    #[test]
    fn external_switch_cancels_without_reactivating_old_window() {
        let now = Instant::now();
        let mut policy = ActivationPolicy::default();
        policy.shown(TARGET, true, now);
        policy.foreground(false);
        policy.foreground(true);
        policy.shown(TARGET, true, now);
        assert!(policy.due(now + Duration::from_secs(1)).is_none());
    }

    #[test]
    fn background_session_is_not_queued_and_duplicate_show_does_not_restart_retries() {
        let now = Instant::now();
        let mut policy = ActivationPolicy::default();
        policy.shown(TARGET, false, now);
        assert!(policy.due(now).is_none());
        policy.destroyed(TARGET.hwnd);
        policy.shown(TARGET, true, now);
        assert_eq!(policy.due(now), Some(TARGET));
        policy.shown(TARGET, true, now);
        assert!(policy.due(now).is_none());
        assert_eq!(policy.due(now + Duration::from_millis(50)), Some(TARGET));
    }

    #[test]
    fn retries_are_bounded_and_destroyed_windows_are_discarded() {
        let now = Instant::now();
        let mut policy = ActivationPolicy::default();
        policy.shown(TARGET, true, now);
        for ms in RETRIES_MS {
            assert_eq!(policy.due(now + Duration::from_millis(ms)), Some(TARGET));
        }
        assert!(policy.exhausted());
        assert!(policy.due(now + Duration::from_secs(2)).is_none());
        policy.destroyed(TARGET.hwnd);
        assert!(policy.pending.is_none());
        policy.shown(TARGET, true, now);
        policy.destroyed(TARGET.hwnd);
        assert!(policy.due(now).is_none());
    }
}
