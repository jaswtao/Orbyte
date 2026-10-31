/** 输入法锚点与组词边界兼容处理（xterm 6）。Author: jaswtao · Date: 2026-10-15 */
import type { IBufferCell, IDisposable, Terminal } from '@xterm/xterm'

const ansiNames = [
  'black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white',
  'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite',
] as const
const ansiDefaults = [
  0x2e3436, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf,
  0x555753, 0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
]

/** Resolve the hex colors used by terminal themes; unknown colors cannot establish a match. */
function themeColor(value: string): number | undefined {
  if (/^#[\da-f]{6}$/i.test(value)) return Number.parseInt(value.slice(1), 16)
  if (/^#[\da-f]{3}$/i.test(value)) return Number.parseInt(value.slice(1).replace(/./g, '$&$&'), 16)
  return undefined
}

function cellColor(cell: IBufferCell, foreground: boolean, options: Terminal['options']): number | undefined {
  const theme = options.theme ?? {}
  const fg = cell.isInverse() ? !foreground : foreground
  let color = fg ? cell.getFgColor() : cell.getBgColor()
  if (fg ? cell.isFgRGB() : cell.isBgRGB()) return color
  if (fg ? cell.isFgDefault() : cell.isBgDefault()) {
    return themeColor(fg ? theme.foreground ?? '#ffffff' : theme.background ?? '#000000')
  }
  if (foreground && color < 8 && cell.isBold() && options.drawBoldTextInBrightColors !== false) color += 8
  if (color < 16) {
    const configured = theme[ansiNames[color]!]
    return configured === undefined ? ansiDefaults[color] : themeColor(configured)
  }
  if (color >= 232) return (8 + (color - 232) * 10) * 0x010101
  const level = [0, 95, 135, 175, 215, 255]
  const index = color - 16
  return (level[Math.floor(index / 36)]! << 16) | (level[Math.floor(index / 6) % 6]! << 8) | level[index % 6]!
}

/** Qoder can draw a black-on-light RGB cursor instead of using ANSI inverse. */
function isRgbCaret(cell: IBufferCell, left: IBufferCell | undefined, right: IBufferCell | undefined, options: Terminal['options']): boolean {
  if (!cell.isFgRGB() || !cell.isBgRGB()) return false
  const fg = cell.getFgColor()
  const bg = cell.getBgColor()
  if (fg === bg) return false
  const neighbors = [left, right].filter((neighbor): neighbor is IBufferCell => neighbor !== undefined)
  const background = neighbors[0] && cellColor(neighbors[0], false, options)
  // Require an isolated background matching the surrounding text. Qoder uses
  // fixed black/white cursor text even when the terminal background differs.
  return background !== undefined && background !== bg
    && (fg === background || fg === 0x000000 || fg === 0xffffff)
    && neighbors.every((neighbor) => cellColor(neighbor, false, options) === background)
    && neighbors.some((neighbor) => cellColor(neighbor, true, options) === bg)
}

/** Find isolated visual cursors, stopping once the position is ambiguous. */
function visualCarets(term: Terminal): { col: number; row: number }[] {
  const buffer = term.buffer.active
  if (typeof buffer.getLine !== 'function') return []
  const start = buffer.viewportY
  const found: { col: number; row: number }[] = []
  for (let y = start; y < start + term.rows; y++) {
    const line = buffer.getLine(y)
    if (!line) continue
    const limit = Math.min(line.length, term.cols)
    for (let x = 0; x < limit; x++) {
      const cell = line.getCell(x)
      if (!cell || (!cell.isInverse() && (!cell.isFgRGB() || !cell.isBgRGB()))) continue
      const width = cell.getWidth()
      if (!width || x + width > limit) continue
      let left = x > 0 ? line.getCell(x - 1) : undefined
      if (left?.getWidth() === 0) left = line.getCell(x - 2)
      const right = x + width < limit ? line.getCell(x + width) : undefined
      // Treat wide characters and their continuation cells as one glyph.
      if (left?.isInverse() || right?.isInverse()) continue
      if (!cell.isInverse() && !isRgbCaret(cell, left, right, term.options)) continue
      found.push({ col: x, row: y - start })
      if (found.length > 1) return found
    }
  }
  return found
}

export function bindTerminalIme(term: Terminal, visible: () => boolean = () => true): IDisposable & { reset(): void } {
  const textarea = term.textarea!
  const screen = term.element!.querySelector<HTMLElement>('.xterm-screen')!
  const composition = term.element!.querySelector<HTMLElement>('.composition-view')!
  let composing = false
  let disposed = false
  let cursorHidden = false
  let pinned: { col: number; row: number } | null = null
  let observer: MutationObserver | undefined
  let frame: number | undefined
  let delayed: ReturnType<typeof setTimeout> | undefined

  const watch = () => {
    observer?.observe(textarea, { attributes: true, attributeFilter: ['style'] })
    observer?.observe(composition, { attributes: true, attributeFilter: ['style'] })
  }
  const sync = () => {
    if (disposed || !visible()) return
    if (!composing && textarea.ownerDocument && textarea.ownerDocument.activeElement !== textarea) return
    if (!screen.clientWidth || !screen.clientHeight) return
    const buffer = term.buffer.active
    // 只有程序隐藏真实光标时才寻找假光标，普通 Shell 反色输出不参与定位。
    const hits = cursorHidden ? visualCarets(term) : []
    if (hits.length === 1) pinned = hits[0]!
    else if (hits.length > 1 || !cursorHidden || !composing) pinned = null
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
      // Collapse advances for narrow and wide glyphs without zeroing the native
      // character height: Windows IMEs need that height to locate the candidate window.
      letterSpacing: composing ? `${-2 * cellWidth}px` : 'normal',
    })
    if (composing) {
      Object.assign(composition.style, {
        left: `${left}px`, top: `${top}px`, maxWidth: `${screen.clientWidth - left}px`,
        overflow: 'hidden',
      })
    }
    watch()
  }
  const schedule = () => {
    if (disposed || !visible() || frame !== undefined) return
    if (typeof requestAnimationFrame === 'undefined') { sync(); return }
    frame = requestAnimationFrame(() => { frame = undefined; sync() })
  }
  // 排在 xterm 的 setTimeout(0) 之后，把被写回硬件光标的坐标再钉到假光标。
  const resync = () => { sync(); clearTimeout(delayed); delayed = setTimeout(sync, 0) }
  const start = () => { composing = true; resync() }
  const end = () => { composing = false; pinned = null; resync() }
  // 捕获阶段先定位；冒泡阶段再覆盖 xterm 按组词长度扩展的输入框。
  textarea.addEventListener('compositionstart', start, true)
  textarea.addEventListener('compositionupdate', resync)
  textarea.addEventListener('compositionend', end)
  textarea.addEventListener('focus', sync)
  const reset = () => { cursorHidden = false; pinned = null; sync() }
  const subscriptions = [
    term.onRender(schedule), term.onCursorMove(schedule), term.onResize(schedule), term.onScroll(schedule),
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
    observer = new MutationObserver(schedule)
    watch()
  }
  sync()
  return { reset, dispose() {
    disposed = true
    clearTimeout(delayed)
    if (frame !== undefined) cancelAnimationFrame(frame)
    observer?.disconnect()
    for (const subscription of subscriptions) subscription.dispose()
    textarea.removeEventListener('compositionstart', start, true)
    textarea.removeEventListener('compositionupdate', resync)
    textarea.removeEventListener('compositionend', end)
    textarea.removeEventListener('focus', sync)
  } }
}
