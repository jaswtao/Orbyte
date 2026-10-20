/** 项目管理 IPC 边界。Author: jaswtao · Date: 2026-10-12 */
import type { ShortcutOverrides } from './workspace/shortcuts'
import { t } from '../i18n/translate'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

export interface ProjectInput {
  name: string
  path: string
  groupId: string | null
  favorite: boolean
}
export interface Project extends ProjectInput {
  id: string
  sortOrder: number
  workspaceId: string
}
export interface ProjectGroup {
  id: string
  name: string
  sortOrder: number
}
export interface Settings {
  shortcuts: ShortcutOverrides
  active_project_id: string | null
  show_workspace_header: boolean
  shell_integration: boolean
  confirm_close_terminal: boolean
  notify_command_seconds: number
  theme: 'dark' | 'light' | 'system'
  font_family: string
  font_size: number
  scrollback: number
  default_shell: 'powershell' | 'cmd' | 'wsl'
  default_directory: string | null
  language: 'en' | 'zh'
}

function projectInput({
  name,
  path,
  groupId,
  favorite,
}: ProjectInput): ProjectInput {
  return { name, path, groupId, favorite }
}

export const projectApi = {
  listProjects: () => invoke<Project[]>('list_projects'),
  createProject: (input: ProjectInput) =>
    invoke<string>('create_project', { input: projectInput(input) }),
  updateProject: (id: string, input: ProjectInput) =>
    invoke<void>('update_project', { id, input: projectInput(input) }),
  deleteProject: (id: string) => invoke<void>('delete_project', { id }),
  listGroups: () => invoke<ProjectGroup[]>('list_groups'),
  createGroup: (name: string) => invoke<string>('create_group', { name }),
  updateGroup: (id: string, name: string) =>
    invoke<void>('update_group', { id, name }),
  deleteGroup: (id: string) => invoke<void>('delete_group', { id }),
  reorderProjects: (entries: [string, string | null][]) =>
    invoke<void>('reorder_projects', { entries }),
  reorderGroups: (ids: string[]) => invoke<void>('reorder_groups', { ids }),
  getSettings: () => invoke<Settings>('get_settings'),
  setSetting: <K extends keyof Settings>(key: K, value: Settings[K]) =>
    invoke<void>('set_setting', { key, value }),
  setSettings: (input: Omit<Settings, 'active_project_id' | 'show_workspace_header'>) =>
    invoke<void>('set_settings', { input }),
  chooseDirectory: (defaultPath?: string) =>
    open({
      directory: true,
      multiple: false,
      defaultPath,
      title: t('settings.chooseDirectory'),
    }),
}
