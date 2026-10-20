/** 三击选行、四击选缓冲区，选区均排除末尾空白。Author: jaswtao · Date: 2026-10-15 */
import type { IBufferRange, IDisposable, Terminal } from '@xterm/xterm'

// xterm 6 的公共 select() 会移除原生拖选监听，因此只在此适配原生选区模型。
interface LineSelectionService {
  _selectLineAt(line: number): void
  _model: {
    selectionStartLength: number
  }
}

function textSelectionLength(term: Terminal, range: IBufferRange): number {
  const buffer = term.buffer.active
  for (let row = range.end.y; row >= range.start.y; row--) {
    const line = buffer.getLine(row)
    if (!line) return 0
    const end = row === range.end.y ? range.end.x : term.cols
    const start = row === range.start.y ? range.start.x : 0
    for (let column = end - 1; column >= start; column--) {
      const cell = line.getCell(column)
      if (!cell || cell.getWidth() === 0 || !/\S/u.test(cell.getChars())) continue
      return (row - range.start.y) * term.cols + column + cell.getWidth() - range.start.x
    }
  }
  return 0
}

export function bindTerminalSelection(term: Terminal): IDisposable {
  const document = term.element!.ownerDocument
  const screen = term.element!.querySelector<HTMLElement>('.xterm-screen')!
  const service = (term as unknown as { _core: { _selectionService: LineSelectionService } })._core._selectionService
  const selectLine = service._selectLineAt
  let originalLength: number | undefined
  const release = () => {
    document.removeEventListener('mousemove', drag, { capture: true })
    document.removeEventListener('mouseup', release)
    originalLength = undefined
  }
  const drag = () => {
    // 真正开始拖动时恢复整行锚点，后续由 xterm 继续处理跨行选择和滚动。
    if (originalLength !== undefined) service._model.selectionStartLength = originalLength
    release()
  }
  const selectTextLine = (rowIndex: number) => {
    release()
    selectLine.call(service, rowIndex)
    const range = term.getSelectionPosition()
    if (!range) return
    originalLength = service._model.selectionStartLength
    document.addEventListener('mousemove', drag, true)
    document.addEventListener('mouseup', release)
    // 在原生 mousedown 调度首次绘制前收紧范围，避免按下与松开之间出现整行高亮。
    service._model.selectionStartLength = textSelectionLength(term, range)
  }
  const selectBuffer = (event: MouseEvent) => {
    if (event.button !== 0 || event.detail !== 4 || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey
      || term.modes.mouseTrackingMode !== 'none') return
    event.preventDefault()
    event.stopPropagation()
    release()
    term.focus()
    const length = textSelectionLength(term, {
      start: { x: 0, y: 0 }, end: { x: term.cols, y: term.buffer.active.length - 1 },
    })
    if (length) term.select(0, 0, length)
    else term.clearSelection()
  }
  service._selectLineAt = selectTextLine
  screen.addEventListener('mousedown', selectBuffer, true)
  return { dispose() {
    release()
    screen.removeEventListener('mousedown', selectBuffer, { capture: true })
    service._selectLineAt = selectLine
  } }
}
