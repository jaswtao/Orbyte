/**
 * 终端相关 Tauri 命令封装。组件不直接调用 invoke。
 *
 * Author: jaswtao
 * Date: 2026-10-09
 */
import { Channel } from '@tauri-apps/api/core'
import { invoke } from './diagnostics'
import type { OutputHealth } from './terminal/recovery'
import type { TerminalFrameData } from './terminal/frame'

export type ShellType = 'powershell' | 'cmd' | 'wsl'

export interface ShellInfo {
  shellType: ShellType
  available: boolean
  program: string | null
  description: string
  unavailableReason: string | null
}

export type TerminalStatus =
  | { state: 'starting' }
  | { state: 'running' }
  | { state: 'exited'; exitCode: number | null }
  | { state: 'failed'; message: string }

export interface TerminalSessionInfo {
  terminalId: string
  terminalConfigId: string | null
  status: TerminalStatus
  output: OutputHealth & { bufferedBytes: number }
}

export interface TerminalSize {
  cols: number
  rows: number
}

export interface TerminalConfig {
  id: string
  workspaceId: string
  name: string
  shellType: ShellType
  cwd: string
  launchPreset: 'codex' | 'claude-code' | null
}

export interface CreatedTerminal {
  terminalConfigId: string
  terminalId: string
}

export interface LaunchPreset { preset: 'codex' | 'claude-code'; description: string; available: boolean; unavailableReason: string | null }

export const terminalApi = {
  reportDiagnostic: (terminalId: string, event: 'frame_error' | 'ack_failed' | 'reconnect_started' | 'reconnect_succeeded' | 'reconnect_failed' | 'poll_failed', generation: number, seq: number | null) =>
    invoke<void>('report_terminal_diagnostic', { terminalId, event, generation, seq }),
  setActiveTerminal: (terminalId: string | null) => invoke<void>('set_active_terminal', { terminalId }),
  readClipboardText: () => invoke<string | null>('read_clipboard_text'),
  writeClipboardText: (text: string) => invoke<void>('write_clipboard_text', { text }),
  saveClipboardImage: () => invoke<string | null>('save_clipboard_image'),
  listLaunchPresets: () => invoke<LaunchPreset[]>('list_launch_presets'),
  exportOutput: (defaultName: string, content: string) => invoke<string | null>('export_terminal_output', { defaultName, content }),
  listAvailableShells: () => invoke<ShellInfo[]>('list_available_shells'),

  createTerminal: (projectId: string, workspaceId: string, shell: ShellType, cwd: string, size: TerminalSize, channel: Channel<TerminalFrameData>, launchPreset: TerminalConfig['launchPreset'] = null) =>
    invoke<CreatedTerminal>('create_terminal', { projectId, workspaceId, shell, cwd, launchPreset, cols: size.cols, rows: size.rows, channel }),

  startTerminal: (terminalConfigId: string, size: TerminalSize, channel: Channel<TerminalFrameData>) =>
    invoke<string>('start_terminal', { terminalConfigId, cols: size.cols, rows: size.rows, channel }),

  removeTerminal: (terminalConfigId: string) => invoke<void>('remove_terminal', { terminalConfigId }),

  listTerminalConfigs: (workspaceId: string) => invoke<TerminalConfig[]>('list_terminal_configs', { workspaceId }),

  listRunningWorkspaces: () => invoke<string[]>('list_running_workspaces'),

  listTerminalSessions: () => invoke<TerminalSessionInfo[]>('list_terminal_sessions'),
  terminalsHaveRunningTasks: (terminalIds: string[]) => invoke<boolean>('terminals_have_running_tasks', { terminalIds }),

  subscribeTerminal: (terminalId: string, channel: Channel<TerminalFrameData>) =>
    invoke<void>('subscribe_terminal', { terminalId, channel }),

  ackTerminal: (terminalId: string, seq: number) => invoke<void>('ack_terminal', { terminalId, seq }),

  pauseOutput: (terminalId: string) => invoke<void>('pause_terminal_output', { terminalId }),

  writeTerminal: (terminalId: string, data: string) => invoke<void>('write_terminal', { terminalId, data }),
  terminalUsesNativePaste: (terminalId: string) => invoke<boolean>('terminal_uses_native_paste', { terminalId }),

  resizeTerminal: (terminalId: string, size: TerminalSize, nudge = false) =>
    invoke<void>('resize_terminal', nudge
      ? { terminalId, cols: size.cols, rows: size.rows, nudge: true }
      : { terminalId, cols: size.cols, rows: size.rows }),

  stopTerminal: (terminalId: string) => invoke<number | null>('stop_terminal', { terminalId }),
}
