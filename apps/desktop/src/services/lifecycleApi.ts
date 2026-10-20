/** 窗口关闭确认 IPC。Author: jaswtao · Date: 2026-10-13 */
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export const lifecycleApi = {
  onCloseRequested: (handler: () => void) => listen('app_close_requested', handler),
  onShutdownStarted: (handler: () => void) => listen('app_shutdown_started', handler),
  resolveClose: (proceed: boolean) => invoke<void>('resolve_close_request', { proceed }),
}
