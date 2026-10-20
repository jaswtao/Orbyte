/** Git IPC。Author: jaswtao · Date: 2026-10-14 */
import { invoke } from '@tauri-apps/api/core'
export type GitKind = 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'conflicted'
export interface GitStatus { isRepo: boolean; branch: string | null; detached: boolean; upstream: string | null; ahead: number; behind: number; files: { path: string; kind: GitKind }[]; truncated: boolean }
export interface Worktree { path: string; branch: string | null; head: string; detached: boolean; isMain: boolean; projectId: string | null }
export const gitApi = {
  available: () => invoke<boolean>('git_available'),
  status: (projectId: string) => invoke<GitStatus>('git_status', { projectId }),
  worktrees: (projectId: string) => invoke<Worktree[]>('git_worktree_list', { projectId }),
  addWorktree: (projectId: string, path: string, branch: string, createBranch: boolean) => invoke<string>('git_worktree_add', { projectId, path, branch, createBranch }),
}
