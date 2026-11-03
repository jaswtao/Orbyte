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

type Anchor = { col: number; row: number }

/** Rows a caret may jump and still follow immediately while composing. */
const PIN_FOLLOW_ROWS = 2
/** Consecutive observations a distant candidate must persist before it replaces the anchor. */
const PIN_STABLE_SAMPLES = 3

export function bindTerminalIme(term: Terminal, visible: () => boolean = () => true): IDisposable & { reset(): void; recover(): void; refresh(): void } {
  const textarea = term.textarea!
  const screen = term.element!.querySelector<HTMLElement>('.xterm-screen')!
  const composition = term.element!.querySelector<HTMLElement>('.composition-view')!
  let composing = false
  let disposed = false
  let cursorHidden = false
  let pinned: Anchor | null = null
  let pinSource: 'visual' | 'hardware' | null = null
  let pending: { anchor: Anchor; samples: number } | null = null
  /** A new composition adopts the current unambiguous caret without settling. */
  let freshStart = false
  let observer: MutationObserver | undefined
  let frame: number | undefined
  let delayed: ReturnType<typeof setTimeout> | undefined

  const hardwareAnchor = (): Anchor | null => {
    const buffer = term.buffer.active
    const row = buffer.baseY + buffer.cursorY - buffer.viewportY
    if (row < 0 || row >= term.rows) return null
    return { col: Math.min(buffer.cursorX, term.cols - 1), row }
  }
  const sameAnchor = (left: Anchor, right: Anchor) => left.col === right.col && left.row === right.row
  /** Prefer candidates on the anchor row: row jumps move the IME window the most. */
  const nearest = (hits: Anchor[], reference: Anchor): Anchor => {
    let best = hits[0]!
    let distance = Math.abs(best.row - reference.row) * term.cols + Math.abs(best.col - reference.col)
    for (const hit of hits.slice(1)) {
      const next = Math.abs(hit.row - reference.row) * term.cols + Math.abs(hit.col - reference.col)
      if (next < distance) { best = hit; distance = next }
    }
    return best
  }
  const clearPin = () => { pinned = null; pinSource = null; pending = null; freshStart = false }

  const watch = () => {
    observer?.observe(textarea, { attributes: true, attributeFilter: ['style'] })
    observer?.observe(composition, { attributes: true, attributeFilter: ['style'] })
  }
  const updatePin = () => {
    const fresh = freshStart
    freshStart = false
    if (!composing) {
      // 只有程序隐藏真实光标时才寻找假光标，普通 Shell 反色输出不参与定位。
      const hits = cursorHidden ? visualCarets(term) : []
      pending = null
      if (hits.length === 1) { pinned = hits[0]!; pinSource = 'visual' }
      else { pinned = null; pinSource = null }
      return
    }
    if (!cursorHidden) { clearPin(); return }

    const hits = visualCarets(term)
    if (!pinned) {
      if (hits.length === 1) { pinned = hits[0]!; pinSource = 'visual'; pending = null }
      else {
        // Freeze the pre-composition position while the TUI redraw is ambiguous;
        // xterm keeps writing the textarea to the moving hardware cursor otherwise.
        pinned = hardwareAnchor()
        pinSource = pinned ? 'hardware' : null
        pending = null
      }
      return
    }
    if (hits.length === 0) { pending = null; return }

    const candidate = hits.length === 1 ? hits[0]! : nearest(hits, pinned)
    const nearby = Math.abs(candidate.row - pinned.row) <= PIN_FOLLOW_ROWS
    // A lone visual caret, or one that stayed on the input line, is unambiguous.
    if ((hits.length === 1 && (fresh || nearby || pinSource === 'hardware')) || (pinSource === 'visual' && nearby)) {
      pinned = candidate
      pinSource = 'visual'
      pending = null
      return
    }
    // A distant candidate may be a spinner or a half-drawn frame: settle before moving.
    if (pending && sameAnchor(pending.anchor, candidate)) pending.samples += 1
    else pending = { anchor: candidate, samples: 1 }
    if (pending.samples >= PIN_STABLE_SAMPLES) {
      pinned = candidate
      pinSource = 'visual'
      pending = null
    }
  }
  const apply = () => {
    if (disposed || !visible()) return
    if (!composing && textarea.ownerDocument && textarea.ownerDocument.activeElement !== textarea) return
    if (!screen.clientWidth || !screen.clientHeight) return
    const anchor = pinned ?? hardwareAnchor()
    if (!anchor || anchor.row < 0 || anchor.row >= term.rows) return
    const { col, row } = anchor
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
  const sync = () => {
    if (disposed || !visible()) return
    if (!composing && textarea.ownerDocument && textarea.ownerDocument.activeElement !== textarea) return
    if (!screen.clientWidth || !screen.clientHeight) return
    updatePin()
    apply()
  }
  /** Re-apply the settled anchor without rescanning, for same-frame corrections. */
  const correct = () => { apply() }
  const schedule = () => {
    if (disposed || !visible() || frame !== undefined) return
    if (typeof requestAnimationFrame === 'undefined') { sync(); return }
    frame = requestAnimationFrame(() => { frame = undefined; sync() })
  }
  // 排在 xterm 的 setTimeout(0) 之后，把被写回硬件光标的坐标再钉到假光标。
  const resync = () => {
    sync()
    clearTimeout(delayed)
    delayed = setTimeout(() => {
      delayed = undefined
      if (composing) correct()
      else sync()
    }, 0)
  }
  const start = () => { composing = true; pending = null; freshStart = true; resync() }
  const end = () => { composing = false; clearPin(); resync() }
  // 捕获阶段先定位；冒泡阶段再覆盖 xterm 按组词长度扩展的输入框。
  textarea.addEventListener('compositionstart', start, true)
  textarea.addEventListener('compositionupdate', resync)
  textarea.addEventListener('compositionend', end)
  textarea.addEventListener('focus', sync)
  const clearPosition = () => {
    clearPin()
    clearTimeout(delayed)
    delayed = undefined
    if (frame !== undefined) cancelAnimationFrame(frame)
    frame = undefined
    sync()
  }
  const reset = () => { cursorHidden = false; composing = false; clearPosition() }
  // xterm.reset() preserves DECTCEM. A resize redraw need not repeat that mode,
  // so keep tracking it while discarding coordinates from the previous screen.
  // Leave an active native composition intact to avoid losing unsent text.
  const recover = () => { clearPosition() }
  const subscriptions = [
    // While composing, xterm schedules its own setTimeout(0) write-backs; correct
    // in the same render instead of leaving the anchor to drift for a frame.
    term.onRender(() => { if (composing) sync(); else schedule() }),
    term.onCursorMove(() => { if (composing) correct(); else schedule() }),
    term.onResize(() => { if (composing) sync(); else schedule() }),
    term.onScroll(() => { if (composing) sync(); else schedule() }),
    // 返回 false，继续交由 xterm 执行原有模式切换和重置。
    term.parser.registerCsiHandler({ prefix: '?', final: 'l' }, (params) => {
      if (params.includes(25)) cursorHidden = true
      return false
    }),
    term.parser.registerCsiHandler({ prefix: '?', final: 'h' }, (params) => {
      if (params.includes(25)) { cursorHidden = false; clearPin() }
      return false
    }),
    term.parser.registerCsiHandler({ intermediates: '!', final: 'p' }, () => { reset(); return false }),
    term.parser.registerEscHandler({ final: 'c' }, () => { reset(); return false }),
  ]
  // 真实 DOM 上 xterm 一改 style 就立刻钉回。测试里的假元素不是 HTMLElement，不挂观察器。
  if (typeof MutationObserver !== 'undefined' && typeof HTMLElement !== 'undefined'
    && textarea instanceof HTMLElement && composition instanceof HTMLElement) {
    // Mutation records run in a microtask, after xterm's zero-delay write-back but
    // before paint, so the IME anchor never lands on the hardware cursor.
    observer = new MutationObserver(() => { if (composing) correct(); else schedule() })
    watch()
  }
  sync()
  return { reset, recover, refresh: () => { if (composing) sync(); else schedule() }, dispose() {
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
