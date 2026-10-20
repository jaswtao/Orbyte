/**
 * 终端相关 Tauri 命令封装。组件不直接调用 invoke。
 *
 * Author: jaswtao
 * Date: 2026-10-09
 */
import { Channel, invoke } from '@tauri-apps/api/core'

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
  setActiveTerminal: (terminalId: string | null) => invoke<void>('set_active_terminal', { terminalId }),
  saveClipboardImage: (png: Uint8Array) => invoke<string>('save_clipboard_image', png),
  listLaunchPresets: () => invoke<LaunchPreset[]>('list_launch_presets'),
  exportOutput: (defaultName: string, content: string) => invoke<string | null>('export_terminal_output', { defaultName, content }),
  listAvailableShells: () => invoke<ShellInfo[]>('list_available_shells'),

  createTerminal: (projectId: string, workspaceId: string, shell: ShellType, cwd: string, size: TerminalSize, channel: Channel<ArrayBuffer>, launchPreset: TerminalConfig['launchPreset'] = null) =>
    invoke<CreatedTerminal>('create_terminal', { projectId, workspaceId, shell, cwd, launchPreset, cols: size.cols, rows: size.rows, channel }),

  startTerminal: (terminalConfigId: string, size: TerminalSize, channel: Channel<ArrayBuffer>) =>
    invoke<string>('start_terminal', { terminalConfigId, cols: size.cols, rows: size.rows, channel }),

  removeTerminal: (terminalConfigId: string) => invoke<void>('remove_terminal', { terminalConfigId }),

  listTerminalConfigs: (workspaceId: string) => invoke<TerminalConfig[]>('list_terminal_configs', { workspaceId }),

  listRunningWorkspaces: () => invoke<string[]>('list_running_workspaces'),

  listTerminalSessions: () => invoke<TerminalSessionInfo[]>('list_terminal_sessions'),
  terminalsHaveRunningTasks: (terminalIds: string[]) => invoke<boolean>('terminals_have_running_tasks', { terminalIds }),

  subscribeTerminal: (terminalId: string, channel: Channel<ArrayBuffer>) =>
    invoke<void>('subscribe_terminal', { terminalId, channel }),

  ackTerminal: (terminalId: string, seq: number) => invoke<void>('ack_terminal', { terminalId, seq }),

  writeTerminal: (terminalId: string, data: string) => invoke<void>('write_terminal', { terminalId, data }),

  resizeTerminal: (terminalId: string, size: TerminalSize) =>
    invoke<void>('resize_terminal', { terminalId, cols: size.cols, rows: size.rows }),

  stopTerminal: (terminalId: string) => invoke<number | null>('stop_terminal', { terminalId }),
}
