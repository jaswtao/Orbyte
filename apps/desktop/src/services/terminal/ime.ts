/** 输入法锚点与组词边界兼容处理（xterm 6）。Author: jaswtao · Date: 2026-10-15 */
import type { IDisposable, Terminal } from '@xterm/xterm'

/** 可见区域最下方的孤立反色字符；宽字符连同占位格作为整体判断。 */
function visualCaret(term: Terminal): { col: number; row: number } | null {
  const buffer = term.buffer.active
  if (typeof buffer.getLine !== 'function') return null
  const start = buffer.viewportY
  let found: { col: number; row: number } | null = null
  for (let y = start; y < start + term.rows; y++) {
    const line = buffer.getLine(y)
    if (!line) continue
    const limit = Math.min(line.length, term.cols)
    for (let x = 0; x < limit; x++) {
      const cell = line.getCell(x)
      if (!cell?.isInverse()) continue
      const width = cell.getWidth()
      if (!width || x + width > limit) continue
      const left = x > 0 ? line.getCell(x - 1) : undefined
      const right = x + width < limit ? line.getCell(x + width) : undefined
      // 跳过宽字符内部的占位格，只检查整个字符两侧，避免把反色高亮条当光标。
      if (left?.isInverse() || right?.isInverse()) continue
      found = { col: x, row: y - start }
    }
  }
  return found
}

export function bindTerminalIme(term: Terminal): IDisposable & { reset(): void } {
  const textarea = term.textarea!
  const screen = term.element!.querySelector<HTMLElement>('.xterm-screen')!
  const composition = term.element!.querySelector<HTMLElement>('.composition-view')!
  let composing = false
  let disposed = false
  let cursorHidden = false
  let pinned: { col: number; row: number } | null = null
  let observer: MutationObserver | undefined

  const watch = () => {
    observer?.observe(textarea, { attributes: true, attributeFilter: ['style'] })
    observer?.observe(composition, { attributes: true, attributeFilter: ['style'] })
  }
  const sync = () => {
    if (disposed) return
    const buffer = term.buffer.active
    // 只有程序隐藏真实光标时才寻找假光标，普通 Shell 反色输出不参与定位。
    const hit = cursorHidden ? visualCaret(term) : null
    if (hit) pinned = hit
    else if (!cursorHidden || !composing) pinned = null
    const col = pinned ? pinned.col : Math.min(buffer.cursorX, term.cols - 1)
    const row = pinned ? pinned.row : buffer.baseY + buffer.cursorY - buffer.viewportY
    if (!screen.clientWidth || !screen.clientHeight || row < 0 || row >= term.rows) return
    const cellWidth = screen.clientWidth / term.cols
    const cellHeight = screen.clientHeight / term.rows
    const left = col * cellWidth
    const top = row * cellHeight
    // 写入期间卸下观察器，避免字体被规范化后再次触发，形成循环。
    observer?.disconnect()
    // 输入框只占一个单元格，避免长拼音把焦点滚动目标推到终端之外。
    Object.assign(textarea.style, {
      left: `${left}px`, top: `${top}px`, width: `${cellWidth}px`,
      maxWidth: `${cellWidth}px`,
      height: `${cellHeight}px`, lineHeight: `${cellHeight}px`,
      fontFamily: term.options.fontFamily, fontSize: `${term.options.fontSize}px`,
    })
    if (composing) {
      Object.assign(composition.style, {
        left: `${left}px`, top: `${top}px`, maxWidth: `${screen.clientWidth - left}px`,
        overflow: 'hidden',
      })
    }
    watch()
  }
  // 排在 xterm 的 setTimeout(0) 之后，把被写回硬件光标的坐标再钉到假光标。
  const resync = () => { sync(); setTimeout(sync, 0) }
  const start = () => { composing = true; resync() }
  const end = () => { composing = false; pinned = null; resync() }
  // 捕获阶段先定位；冒泡阶段再覆盖 xterm 按组词长度扩展的输入框。
  textarea.addEventListener('compositionstart', start, true)
  textarea.addEventListener('compositionupdate', resync)
  textarea.addEventListener('compositionend', end)
  textarea.addEventListener('focus', sync)
  const reset = () => { cursorHidden = false; pinned = null; sync() }
  const subscriptions = [
    term.onRender(sync), term.onCursorMove(sync), term.onResize(sync), term.onScroll(sync),
    // 返回 false，继续交由 xterm 执行原有模式切换和重置。
    term.parser.registerCsiHandler({ prefix: '?', final: 'l' }, (params) => {
      if (params.includes(25)) cursorHidden = true
      return false
    }),
    term.parser.registerCsiHandler({ prefix: '?', final: 'h' }, (params) => {
      if (params.includes(25)) { cursorHidden = false; pinned = null }
      return false
    }),
    term.parser.registerCsiHandler({ intermediates: '!', final: 'p' }, () => { reset(); return false }),
    term.parser.registerEscHandler({ final: 'c' }, () => { reset(); return false }),
  ]
  // 真实 DOM 上 xterm 一改 style 就立刻钉回。测试里的假元素不是 HTMLElement，不挂观察器。
  if (typeof MutationObserver !== 'undefined' && typeof HTMLElement !== 'undefined'
    && textarea instanceof HTMLElement && composition instanceof HTMLElement) {
    observer = new MutationObserver(() => sync())
    watch()
  }
  sync()
  return { reset, dispose() {
    disposed = true
    observer?.disconnect()
    for (const subscription of subscriptions) subscription.dispose()
    textarea.removeEventListener('compositionstart', start, true)
    textarea.removeEventListener('compositionupdate', resync)
    textarea.removeEventListener('compositionend', end)
    textarea.removeEventListener('focus', sync)
  } }
}
