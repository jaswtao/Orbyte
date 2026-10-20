/** 从 xterm 缓冲还原折行并限制 UTF-8 大小。Author: jaswtao · Date: 2026-10-14 */
import type { IBuffer } from '@xterm/xterm'
import { t } from '../../i18n/translate'
export function exportBuffer(buffer: Pick<IBuffer, 'length' | 'getLine'>, limit = 50 * 1024 * 1024): string {
  const parts: string[] = []
  let bytes = 0
  let truncated = false
  const encoder = new TextEncoder()
  for (let i = buffer.length - 1; i >= 0; i--) {
    const line = buffer.getLine(i)
    if (!line) continue
    const text = line.translateToString(true) + (i < buffer.length - 1 && !buffer.getLine(i + 1)?.isWrapped ? '\r\n' : '')
    const size = encoder.encode(text).length
    if (bytes + size > limit - 100) { truncated = true; break }
    bytes += size; parts.push(text)
  }
  return (truncated ? t('terminal.exportTruncated') : '') + parts.reverse().join('')
}
export function outputFilename(project: string, tab: string, date = new Date()): string {
  const stamp = `${date.getFullYear()}${String(date.getMonth() + 1).padStart(2, '0')}${String(date.getDate()).padStart(2, '0')}-${String(date.getHours()).padStart(2, '0')}${String(date.getMinutes()).padStart(2, '0')}${String(date.getSeconds()).padStart(2, '0')}`
  return `${project}-${tab}-${stamp}`.replace(/[<>:"/\\|?*\x00-\x1f]/g, '_').slice(0, 180) + '.txt'
}
