/** 比较终端解析后的文字，忽略光标、样式与相同内容重绘。Author: jaswtao · Date: 2026-10-15 */
import type { IBuffer, IMarker, Terminal } from '@xterm/xterm'

interface PromptRegion {
  marker: IMarker
  prefix: string
  text: string | null
  end: number | null
}

export class ContentActivity {
  private previous = ''
  private prompts: PromptRegion[] = []
  private pending: PromptRegion | null = null

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
    const parts: string[] = []
    this.prompts = this.prompts.filter((prompt) => !prompt.marker.isDisposed)
    const prompts = new Map(this.prompts.map((prompt) => [prompt.marker.line, prompt]))
    for (let index = 0; index < buffer.length; index++) {
      const start = index
      let text = ''
      while (true) {
        const nextWrapped = buffer.getLine(index + 1)?.isWrapped === true
        text += buffer.getLine(index)?.translateToString(!nextWrapped) ?? ''
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
    const changed = current !== this.previous
    this.previous = current
    return changed
  }
}
