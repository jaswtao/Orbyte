/** Compares parsed terminal text while ignoring cursor, styles, identical repaints, and small status refreshes on the alternate screen. Author: jaswtao · Date: 2026-10-15 */
import type { IBuffer, IMarker, Terminal } from '@xterm/xterm'

interface PromptRegion {
  marker: IMarker
  prefix: string
  text: string | null
  end: number | null
}

export class ContentActivity {
  private previous = ''
  private lastRead = ''
  private prompts: PromptRegion[] = []
  private pending: PromptRegion | null = null

  get unread(): boolean {
    return this.previous !== this.lastRead
  }

  markRead(): void {
    this.lastRead = this.previous
  }

  /** 在解析 OSC 的位置记录区域，marker 随滚动与软换行重排移动。 */
  promptStart(term: Terminal): void {
    const buffer = term.buffer.active
    if (buffer.type !== 'normal') return
    let row = buffer.baseY + buffer.cursorY
    let prefix = buffer.getLine(row)?.translateToString(false, 0, buffer.cursorX) ?? ''
    while (row > 0 && buffer.getLine(row)?.isWrapped) {
      row--
      prefix = (buffer.getLine(row)?.translateToString(false) ?? '') + prefix
    }
    const marker = term.registerMarker(row - buffer.baseY - buffer.cursorY)
    if (!marker) return
    // PSReadLine 可能在原位置重新发送提示符。
    this.prompts = this.prompts.filter((prompt) => {
      if (prompt.marker.isDisposed) return false
      if (prompt.marker.line !== row) return true
      prompt.marker.dispose()
      return false
    })
    this.pending = { marker, prefix, text: null, end: null }
    this.prompts.push(this.pending)
  }

  promptEnd(term: Terminal): void {
    if (!this.pending) return
    this.pending.text = this.textToCursor(term, this.pending.marker.line).slice(this.pending.prefix.length).trimEnd()
    this.pending = null
  }

  /** 显式 OSC C 可结束同一行的输入区；无 C 的 PowerShell 以回显换行结束。 */
  commandStart(term: Terminal): void {
    const prompt = this.prompts.at(-1)
    if (prompt && !prompt.marker.isDisposed && term.buffer.active.type === 'normal') {
      prompt.end = this.textToCursor(term, prompt.marker.line).length
    }
  }

  /** xterm 会截短光标所在的长行，保留重排前后共同的提示符前缀。 */
  resized(buffer: IBuffer): void {
    if (buffer.type !== 'normal') return
    for (const prompt of this.prompts) {
      if (prompt.marker.isDisposed || prompt.text === null) continue
      let text = ''
      let row = prompt.marker.line
      do {
        const wrapped = buffer.getLine(row + 1)?.isWrapped === true
        text += buffer.getLine(row)?.translateToString(!wrapped) ?? ''
        if (!wrapped) break
        row++
      } while (row < buffer.length)
      if (!text.startsWith(prompt.prefix)) continue
      const current = text.slice(prompt.prefix.length)
      let length = 0
      while (length < prompt.text.length && prompt.text[length] === current[length]) length++
      if (length > 0) prompt.text = prompt.text.slice(0, length)
    }
  }

  reset(buffer?: Pick<IBuffer, 'length' | 'getLine' | 'type'>): void {
    for (const prompt of this.prompts) prompt.marker.dispose()
    this.prompts = []
    this.pending = null
    this.previous = ''
    if (buffer) this.changed(buffer)
    this.markRead()
  }

  private textToCursor(term: Terminal, start: number): string {
    const buffer = term.buffer.active
    const row = buffer.baseY + buffer.cursorY
    let text = ''
    for (let index = start; index <= row; index++) {
      text += buffer.getLine(index)?.translateToString(false, 0, index === row ? buffer.cursorX : undefined) ?? ''
    }
    return text
  }

  changed(buffer: Pick<IBuffer, 'length' | 'getLine' | 'type'>): boolean {
    const scan = this.scan(buffer)
    let result = scan.next()
    while (!result.done) result = scan.next()
    return result.value
  }

  /** Yield between rows so history comparison shares the terminal parsing budget. */
  *scan(buffer: Pick<IBuffer, 'length' | 'getLine' | 'type'>): Generator<void, boolean> {
    const parts: string[] = []
    this.prompts = this.prompts.filter((prompt) => !prompt.marker.isDisposed)
    const prompts = new Map(this.prompts.map((prompt) => [prompt.marker.line, prompt]))
    for (let index = 0; index < buffer.length; index++) {
      const start = index
      let text = ''
      while (true) {
        const nextWrapped = buffer.getLine(index + 1)?.isWrapped === true
        text += buffer.getLine(index)?.translateToString(!nextWrapped) ?? ''
        if (index % 32 === 31) yield
        if (!nextWrapped) break
        index++
      }
      const prompt = buffer.type === 'alternate' ? undefined : prompts.get(start)
      // xterm 调整尺寸时可能截短光标所在行，仍视为原提示符。
      const promptText = prompt && text.startsWith(prompt.prefix) ? text.slice(prompt.prefix.length) : null
      if (prompt && promptText !== null && (prompt.text === null || promptText.startsWith(prompt.text) || prompt.text.startsWith(promptText.trimEnd()))) {
        text = prompt.prefix + (prompt.end === null ? '' : text.slice(prompt.end))
        if (!text) continue
      }
      parts.push(text + '\n')
    }
    // 还原软换行，忽略终端为视口预留的空行，避免尺寸调整产生提示。
    const current = parts.join('').replace(/\n+$/, '')
    // Compare with the last read screen so in-place quota or cursor updates do not accumulate unread; letter changes still count as new content.
    if (buffer.type === 'alternate' && current !== this.lastRead && isMinorAlternateRefresh(this.lastRead, current)) {
      this.previous = this.lastRead
      return false
    }
    const changed = current !== this.previous
    this.previous = current
    return changed
  }
}

/** Character budget for in-place refreshes. A status line changing a number stays within it. */
const MINOR_ALTERNATE_CHARS = 24

/** Treat it as a status refresh only when the alternate screen keeps its line count and letters and only a few characters change in place. */
function isMinorAlternateRefresh(baseline: string, current: string): boolean {
  if (!baseline) return false
  const before = baseline.split('\n')
  const after = current.split('\n')
  if (before.length !== after.length || before.length < 2) return false
  let distance = 0
  let stable = 0
  for (let index = 0; index < before.length; index++) {
    if (before[index] === after[index]) { stable++; continue }
    // Letters carry content: "Working..." -> "Approve? (y/N)" must notify; digits, symbols, and spinner glyphs are status values.
    if (lettersOnly(before[index]) !== lettersOnly(after[index])) return false
    distance += editDistance(before[index], after[index], MINOR_ALTERNATE_CHARS - distance + 1)
    if (distance > MINOR_ALTERNATE_CHARS) return false
  }
  return stable >= 1
}

function lettersOnly(text: string): string {
  return text.replace(/[^\p{L}]/gu, '')
}

function editDistance(left: string, right: string, cap: number): number {
  let start = 0
  while (start < left.length && start < right.length && left.charCodeAt(start) === right.charCodeAt(start)) start++
  let leftEnd = left.length
  let rightEnd = right.length
  while (leftEnd > start && rightEnd > start && left.charCodeAt(leftEnd - 1) === right.charCodeAt(rightEnd - 1)) {
    leftEnd--
    rightEnd--
  }
  left = left.slice(start, leftEnd)
  right = right.slice(start, rightEnd)
  if (Math.abs(left.length - right.length) >= cap || left.length > 400 || right.length > 400) return cap
  let previous = Array.from({ length: right.length + 1 }, (_, index) => index)
  for (let i = 1; i <= left.length; i++) {
    const next = [i]
    let best = i
    const code = left.charCodeAt(i - 1)
    for (let j = 1; j <= right.length; j++) {
      const cost = code === right.charCodeAt(j - 1) ? 0 : 1
      const value = Math.min(previous[j] + 1, next[j - 1] + 1, previous[j - 1] + cost)
      next[j] = value
      if (value < best) best = value
    }
    if (best >= cap) return cap
    previous = next
  }
  return previous[right.length]
}
