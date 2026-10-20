/** 解析不可信 OSC；只生成显示与计时事件。Author: jaswtao · Date: 2026-10-14 */
export type ShellEvent = { type: 'promptStart' | 'promptEnd' | 'commandStart' } | { type: 'commandEnd'; exitCode?: number } | { type: 'cwd'; path: string }

export class ShellIntegration {
  private waiting = false
  constructor(private readonly emit: (event: ShellEvent) => void) {}
  reset() { this.waiting = false }
  input(data: string) {
    if (this.waiting && data.includes('\r')) { this.waiting = false; this.emit({ type: 'commandStart' }) }
  }
  osc(code: number, data: string): boolean {
    if (data.length > 1024 || /[\x00-\x1f\x7f-\x9f]/.test(data)) return true
    if (code === 133) {
      const [kind, exit] = data.split(';')
      if (kind === 'A') { this.waiting = false; this.emit({ type: 'promptStart' }) }
      if (kind === 'B') { this.waiting = true; this.emit({ type: 'promptEnd' }) }
      if (kind === 'C') { this.waiting = false; this.emit({ type: 'commandStart' }) }
      if (kind === 'D') { this.waiting = false; this.emit({ type: 'commandEnd', exitCode: exit !== undefined && /^-?\d+$/.test(exit) && Number.isSafeInteger(Number(exit)) ? Number(exit) : undefined }) }
    } else if (code === 9 && data.startsWith('9;')) {
      if (data.length > 2) this.emit({ type: 'cwd', path: data.slice(2) })
    } else if (code === 7) {
      try {
        const url = new URL(data)
        if (url.protocol !== 'file:') return true
        let path = decodeURIComponent(url.pathname)
        if (/^\/[A-Za-z]:\//.test(path)) path = path.slice(1)
        if (url.hostname && url.hostname !== 'localhost') path = `//${url.hostname}${path}`
        if (!/[\x00-\x1f\x7f-\x9f]/.test(path)) this.emit({ type: 'cwd', path })
      } catch { /* 非法 URI 不改变目录。 */ }
    }
    return true
  }
}
