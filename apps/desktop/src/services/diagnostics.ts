/** Application diagnostics without command payloads. Author: jaswtao · Date: 2026-10-23 */
import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import type { InvokeArgs, InvokeOptions } from '@tauri-apps/api/core'
import type { App } from 'vue'

const quietCommands = new Set([
  'ack_terminal', 'write_terminal', 'resize_terminal', 'set_active_terminal',
  'list_terminal_sessions', 'list_running_workspaces', 'terminals_have_running_tasks',
  'put_editor_draft', 'delete_editor_draft',
  'git_status', 'set_unsaved_editor_count', 'report_terminal_diagnostic',
  'list_remote_transfers', 'list_remote_terminal_sessions', 'submit_ssh_credential',
])
let sequence = 0
const page = crypto.randomUUID()

function report(event: string, command: string, outcome: string, elapsedMs = 0, context = '') {
  // Use the raw transport to prevent recursive logging if diagnostics itself fails.
  void tauriInvoke('report_app_diagnostic', {
    event, command, outcome, elapsedMs: Math.round(elapsedMs), context,
  }).catch(() => {})
}

export async function invoke<T>(command: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  const started = performance.now()
  const request = `${page}:${++sequence}`
  const ids = Object.entries(args ?? {}).filter(([key, value]) =>
    /^(projectId|workspaceId|terminalId|terminalConfigId|id)$/.test(key)
    && typeof value === 'string' && /^[a-f0-9-]{36}$/i.test(value),
  ).map(([key, value]) => `${key}=${value}`).join(' ')
  const context = `request=${request} ${ids}`.trim()
  const quiet = quietCommands.has(command)
  if (!quiet) report('command_started', command, 'pending', 0, context)
  try {
    const result = await tauriInvoke<T>(command, args, options)
    const elapsed = performance.now() - started
    if (!quiet || elapsed >= 1000) report('command_finished', command, 'ok', elapsed, context)
    return result
  } catch (error) {
    // Free-form errors can echo file/terminal content. Store only a known error category.
    const kind = typeof error === 'object' && error !== null && 'kind' in error
      && ['message', 'missing', 'conflict', 'unmappable'].includes(String(error.kind))
      ? String(error.kind) : 'unknown'
    report('command_finished', command, 'error', performance.now() - started, `${context} kind=${kind}`)
    throw error
  }
}

function errorLocation(error: unknown): string {
  if (!(error instanceof Error)) return ''
  // Bundle coordinates identify the failing code without copying messages or local paths.
  return (error.stack?.split('\n').slice(1).join('\n').match(/[\w-]+\.js:\d+:\d+/g) ?? []).slice(0, 8).join(' ')
}

export function installDiagnostics() {
  window.addEventListener('error', (event) => {
    // Keep source coordinates, excluding error messages, source URLs and arbitrary rejection values.
    report('window_error', '', 'error', 0, `line=${event.lineno ?? 0} column=${event.colno ?? 0} ${errorLocation(event.error)}`)
  })
  window.addEventListener('unhandledrejection', (event) => report('unhandled_rejection', '', 'error', 0, errorLocation(event.reason)))
  report('frontend_ready', '', 'ok')
}

export function attachVueDiagnostics(app: App) {
  app.config.errorHandler = (error, _instance, info) => {
    report('vue_error', '', 'error', 0, `${info.slice(0, 128)} ${errorLocation(error)}`)
  }
  return app
}
