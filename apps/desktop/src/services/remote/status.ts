/**
 * Connection and SFTP readiness shared by the sidebar and the open workspace.
 * Passwords are never stored here.
 *
 * Author: jaswtao
 * Date: 2026-10-31
 */
import { reactive } from 'vue'

export interface RemoteLinkState {
  state: string
  message: string
}

export const remoteLinks = reactive<Record<string, RemoteLinkState>>({})
const sftpReady = new Set<string>()

export function setRemoteLink(id: string, state: string, message = '') {
  remoteLinks[id] = { state, message }
}

export function clearRemoteLink(id: string) {
  delete remoteLinks[id]
}

export function markSftpReady(workspaceId: string, ready: boolean) {
  if (ready) sftpReady.add(workspaceId)
  else sftpReady.delete(workspaceId)
}

export function isSftpReady(workspaceId: string): boolean {
  return sftpReady.has(workspaceId)
}
