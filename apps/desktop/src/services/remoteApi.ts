/**
 * Remote connection and SFTP commands. Components do not call invoke directly.
 *
 * Author: jaswtao
 * Date: 2026-10-31
 */
import { Channel } from '@tauri-apps/api/core'
import { invoke } from './diagnostics'
import type { TerminalFrameData } from './terminal/frame'
import type { TerminalSessionInfo, TerminalSize } from './terminalApi'

export interface SshConnection {
  id: string
  name: string
  host: string
  port: number
  username: string
  authType: 'password' | 'privateKey'
  privateKeyPath: string | null
  sortOrder: number
  hasSavedPassword: boolean
}

export type PasswordChange = { operation: 'keep' | 'clear' } | { operation: 'set'; password: string }
export type SshConnectionInput = Omit<SshConnection, 'id' | 'sortOrder' | 'hasSavedPassword'> & { passwordChange?: PasswordChange }

export interface KnownHost {
  id: string
  host: string
  port: number
  algorithm: string
  fingerprintSha256: string
}

export interface RemoteTerminalConfig {
  id: string
  remoteWorkspaceId: string
  name: string
  sortOrder: number
}

export interface RemoteLayoutNode {
  type: 'terminal' | 'split'
  terminalConfigId?: string
  direction?: 'horizontal' | 'vertical'
  ratio?: number
  children?: RemoteLayoutNode[]
}

export interface RemoteTab {
  id: string
  title: string
  root: RemoteLayoutNode
}

export interface RemoteWorkspace {
  id: string
  connectionId: string
  layout: { schemaVersion: number; activeTabId: string | null; tabs: RemoteTab[] }
  lastDirectory: string | null
  terminals: RemoteTerminalConfig[]
}

export interface SshUiEvent {
  type: 'challenge' | 'credentialRequired' | 'attempt'
  challengeId?: string
  host?: string
  port?: number
  algorithm?: string
  fingerprint?: string
  batchId?: string
  authType?: string
  attemptId?: string
  terminalId?: string | null
  state?: string
  message?: string | null
}

export interface DirEntry {
  name: string
  kind: string
  size: number
  mtime: number | null
}

export interface DirBatch {
  requestId: string
  path: string
  entries: DirEntry[]
  done: boolean
  message: string | null
}

export interface TransferFile {
  relative: string
  status: string
  message: string | null
  bytes: number
  total: number | null
}

export interface TransferView {
  id: string
  clientRequestId: string
  workspaceId: string
  direction: string
  state: string
  transferred: number
  total: number | null
  bytesPerSecond: number
  message: string | null
  files: TransferFile[]
  revision: number
}

export const remoteApi = {
  listConnections: () => invoke<SshConnection[]>('list_ssh_connections'),
  createConnection: (input: SshConnectionInput) => invoke<string>('create_ssh_connection', {
    name: input.name, host: input.host, port: input.port, username: input.username, authType: input.authType, privateKeyPath: input.privateKeyPath, passwordChange: input.passwordChange,
  }),
  updateConnection: (id: string, input: SshConnectionInput) => invoke<void>('update_ssh_connection', {
    id, name: input.name, host: input.host, port: input.port, username: input.username, authType: input.authType, privateKeyPath: input.privateKeyPath, passwordChange: input.passwordChange,
  }),
  deleteConnection: (id: string) => invoke<void>('delete_ssh_connection', { id }),
  listKnownHosts: (host: string, port: number) => invoke<KnownHost[]>('list_ssh_known_hosts', { host, port }),
  forgetHostKey: (id: string) => invoke<void>('forget_ssh_host_key', { id }),
  loadWorkspace: (connectionId: string) => invoke<RemoteWorkspace>('load_remote_workspace', { connectionId }),
  saveWorkspace: (workspaceId: string, layout: RemoteWorkspace['layout']) => invoke<void>('save_remote_workspace', { workspaceId, layout }),
  setDirectory: (workspaceId: string, directory: string) => invoke<void>('set_remote_directory', { workspaceId, directory }),
  createTerminal: (workspaceId: string, name: string) => invoke<string>('create_remote_terminal_config', { workspaceId, name }),
  removeTerminal: (workspaceId: string, terminalConfigId: string) => invoke<void>('remove_remote_terminal_config', { workspaceId, terminalConfigId }),
  beginConnect: (connectionId: string, events: Channel<SshUiEvent>) => invoke<{ batchId: string }>('begin_ssh_connect', { connectionId, events }),
  sealConnect: (batchId: string) => invoke<void>('seal_ssh_connect', { batchId }),
  connectTerminal: (batchId: string, connectionId: string, configId: string, size: TerminalSize, channel: Channel<TerminalFrameData>) =>
    invoke<{ terminalId: string; attemptId: string }>('connect_ssh_terminal', { batchId, connectionId, configId, cols: size.cols, rows: size.rows, channel }),
  connectSftp: (batchId: string, connectionId: string, workspaceId: string) =>
    invoke<{ attemptId: string }>('connect_sftp', { batchId, connectionId, workspaceId }),
  answerChallenge: (challengeId: string, decision: 'once' | 'save' | 'cancel') => invoke<void>('answer_ssh_challenge', { challengeId, decision }),
  submitCredential: (batchId: string, secret: string, savePassword = false) => invoke<void>('submit_ssh_credential', { batchId, secret, savePassword }),
  cancelAttempt: (attemptId: string) => invoke<void>('cancel_ssh_attempt', { attemptId }),
  listSessions: () => invoke<TerminalSessionInfo[]>('list_remote_terminal_sessions'),
  listDirectory: (workspaceId: string, path: string, events: Channel<DirBatch>) => invoke<string>('list_remote_directory', { workspaceId, path, events }),
  cancelBrowse: (requestId: string) => invoke<void>('cancel_remote_browse', { requestId }),
  createDirectory: (workspaceId: string, path: string) => invoke<void>('create_remote_directory', { workspaceId, path }),
  canonicalize: (workspaceId: string, path: string) => invoke<string>('canonicalize_remote_path', { workspaceId, path }),
  selectPaths: (workspaceId: string, kind: 'upload-files' | 'upload-folders' | 'download') =>
    invoke<{ token: string; count: number; labels: string[] } | null>('select_transfer_paths', { workspaceId, kind }),
  enqueueTransfer: (workspaceId: string, token: string, clientRequestId: string, remoteDirectory: string, remoteNames: string[]) =>
    invoke<string>('enqueue_remote_transfer', { workspaceId, token, clientRequestId, remoteDirectory, remoteNames }),
  listTransfers: () => invoke<{ revision: number; transfers: TransferView[] }>('list_remote_transfers'),
  subscribeTransfers: (events: Channel<{ transfers: TransferView[] }>) => invoke<number>('subscribe_remote_transfers', { events }),
  cancelTransfer: (transferId: string) => invoke<void>('cancel_remote_transfer', { transferId }),
  resolveConflict: (transferId: string, choice: 'overwrite' | 'skip' | 'cancel', applyAll: boolean) =>
    invoke<void>('resolve_transfer_conflict', { transferId, choice, applyAll }),
}
