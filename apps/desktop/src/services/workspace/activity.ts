/** 终端与项目任务标记优先级。Author: jaswtao · Date: 2026-10-14 */
export type Activity = 'failed' | 'unread' | null
export interface ActivityEntry {
  status: { state: string; exitCode?: number | null }
  shell: { command: string; lastExitCode: number | null }
  unread: boolean
}
export function activity(entries: ActivityEntry[]): Activity {
  // 命令失败标记保持到下一条命令开始（commandStart 会清空 lastExitCode）。
  if (entries.some((e) => e.status.state === 'failed' || (e.status.state === 'exited' && e.status.exitCode != null && e.status.exitCode !== 0) || (e.shell.lastExitCode != null && e.shell.lastExitCode !== 0 && e.shell.command !== 'running'))) return 'failed'
  return entries.some((e) => e.unread) ? 'unread' : null
}
