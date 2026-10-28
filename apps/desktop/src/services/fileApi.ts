/** 项目文件 IPC。Author: jaswtao · Date: 2026-10-13 */
import { Channel } from '@tauri-apps/api/core'
import { invoke } from './diagnostics'
export type Encoding = 'utf-8' | 'gbk'
export interface TextFile { content: string; version: string; encoding: Encoding; bom: boolean; lineEnding: 'LF' | 'CRLF' }
export interface EditorDraft { projectId: string; path: string; projectPath: string; content: string; baseline: string; encoding: Encoding; bom: boolean; lineEnding: 'LF' | 'CRLF'; revision: string; updatedAt: string }
export interface FileError { kind: 'message' | 'missing' | 'conflict' | 'unmappable'; message: string; position?: number; length?: number }
export interface DirectoryEntry { name: string; path: string; isDirectory: boolean; accessible: boolean }
export type FileChange = { paths: string[] } | { rescan: true }
export const fileError = (reason: unknown): FileError => typeof reason === 'object' && reason !== null && 'kind' in reason ? reason as FileError : { kind: 'message', message: String(reason) }
export const fileApi = {
  drafts: () => invoke<EditorDraft[]>('list_editor_drafts'),
  putDraft: (draft: EditorDraft) => invoke<void>('put_editor_draft', { draft }),
  deleteDraft: (projectId: string, path: string, revision: string) => invoke<void>('delete_editor_draft', { projectId, path, revision }),
  renameDrafts: (projectId: string, from: string, to: string) => invoke<void>('rename_editor_drafts', { projectId, from, to }),
  list: (projectId: string, path = '') => invoke<{ entries: DirectoryEntry[]; truncated: boolean }>('list_directory', { projectId, path }),
  read: (projectId: string, path: string, encoding: Encoding | null = null) => invoke<TextFile>('read_file', { projectId, path, encoding }),
  write: (projectId: string, path: string, request: Omit<TextFile, 'version'> & { expectedVersion: string | null }) => invoke<TextFile>('write_file', { projectId, path, request }),
  create: (projectId: string, path: string, directory: boolean) => invoke<void>(directory ? 'create_directory' : 'create_file', { projectId, path }),
  rename: (projectId: string, from: string, to: string) => invoke<void>('rename_path', { projectId, from, to }),
  delete: (projectId: string, path: string) => invoke<void>('delete_path', { projectId, path }),
  search: (projectId: string, query: string, includeIgnored = false) => invoke<{ paths: string[]; incomplete: boolean }>('search_files', { projectId, query, includeIgnored }),
  async watch(projectId: string, callback: (event: FileChange) => void) {
    const channel = new Channel<FileChange>()
    channel.onmessage = callback
    await invoke('watch_project_files', { projectId, onEvent: channel })
    return channel
  },
  unsaved: (count: number) => invoke<void>('set_unsaved_editor_count', { count }),
}
