/**
 * Pure helpers for remote workspaces. SSH sessions stay on the Rust side.
 *
 * Author: jaswtao
 * Date: 2026-10-31
 */

export interface RemoteSessionSnapshot {
  terminalId: string
  terminalConfigId: string | null
  status: { state: string }
}

export function endpointLabel(username: string, host: string, port: number): string {
  return `${username}@${host}:${port}`
}

/** Closing a remote pane always asks. The local terminal confirm setting does not apply. */
export function remotePaneNeedsCloseConfirm(): boolean {
  return true
}

export function batchCancelId(batchId: string): string {
  return `batch:${batchId}`
}

/** Scanning has no reliable total, so it must not be shown as a percentage. */
export function transferPercent(state: string, transferred: number, total: number | null): number | null {
  if (state === 'queued' || state === 'scanning' || total == null || total <= 0) return null
  return Math.min(100, Math.round((transferred / total) * 100))
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1
  return `${value.toFixed(digits)} ${units[unit]}`
}

export function parentPath(path: string): string | null {
  if (!path || path === '/') return null
  const trimmed = path.endsWith('/') ? path.slice(0, -1) : path
  const index = trimmed.lastIndexOf('/')
  if (index < 0) return null
  return index === 0 ? '/' : trimmed.slice(0, index)
}

export function joinRemote(directory: string, name: string): string {
  if (!name || name.includes('/') || name.includes('\0')) return directory
  if (directory === '/' || directory === '') return `/${name}`
  return `${directory.replace(/\/$/, '')}/${name}`
}

export function partitionSessions(configIds: string[], sessions: RemoteSessionSnapshot[]) {
  const attach: { configId: string; terminalId: string }[] = []
  const connect: string[] = []
  for (const configId of configIds) {
    const session = sessions.find((item) => item.terminalConfigId === configId && (item.status.state === 'running' || item.status.state === 'starting'))
    if (session) attach.push({ configId, terminalId: session.terminalId })
    else connect.push(configId)
  }
  return { attach, connect }
}

export function failedRoots(files: { relative: string; status: string }[]): string[] {
  const names = new Set<string>()
  for (const file of files) {
    if (file.status !== 'failed' && file.status !== 'cancelled') continue
    const root = file.relative.split('/').find((part) => part.length > 0)
    if (root && !root.includes('\0')) names.add(root)
  }
  return [...names]
}
