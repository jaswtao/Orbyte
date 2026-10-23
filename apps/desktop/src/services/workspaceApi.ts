/** 布局 IPC。Author: jaswtao · Date: 2026-10-13 */
import { invoke } from './diagnostics'
import type { Layout } from './workspace/layout'
export const workspaceApi = {
  load: (workspaceId: string) => invoke<Layout>('load_workspace_layout', { workspaceId }),
  save: (workspaceId: string, layout: Layout) => invoke<void>('save_workspace_layout', { workspaceId, layoutJson: JSON.stringify(layout) }),
}
