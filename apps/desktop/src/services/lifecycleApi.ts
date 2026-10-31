/** 窗口关闭确认 IPC。Author: jaswtao · Date: 2026-10-13 */
import { invoke } from './diagnostics'
import { listen } from '@tauri-apps/api/event'

export interface ClosePrompt {
  unsaved?: boolean
  transfers?: boolean
}

export const lifecycleApi = {
  onCloseRequested: (handler: (event: { payload: ClosePrompt }) => void) => listen<ClosePrompt>('app_close_requested', handler),
  onShutdownStarted: (handler: () => void) => listen('app_shutdown_started', handler),
  resolveClose: (proceed: boolean) => invoke<void>('resolve_close_request', { proceed }),
}
