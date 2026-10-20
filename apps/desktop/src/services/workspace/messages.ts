/** 工作区浮层消息，与业务错误状态独立。Author: jaswtao · Date: 2026-10-16 */
import { shallowRef } from 'vue'

export interface WorkspaceMessage {
  type: 'error' | 'warning'
  content: string
}

export const workspaceMessage = shallowRef<WorkspaceMessage | null>(null)

export function showWorkspaceMessage(content: string, type: WorkspaceMessage['type'] = 'error'): void {
  if (content) workspaceMessage.value = { type, content }
}
