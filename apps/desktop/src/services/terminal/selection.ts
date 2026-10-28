/** Trim line selection at both ends and buffer selection at the end. Author: jaswtao · Date: 2026-10-28 */
import type { IBufferRange, IDisposable, Terminal } from '@xterm/xterm'

// xterm 6 的公共 select() 会移除原生拖选监听，因此只在此适配原生选区模型。
interface LineSelectionService {
  _selectLineAt(line: number): void
  _model: {
    selectionStart: [number, number] | undefined
    selectionStartLength: number
  }
}

function textSelectionStart(term: Terminal, range: IBufferRange): IBufferRange['start'] {
  const buffer = term.buffer.active
  for (let row = range.start.y; row <= range.end.y; row++) {
    const line = buffer.getLine(row)
    if (!line) return range.start
    const start = row === range.start.y ? range.start.x : 0
    const end = row === range.end.y ? range.end.x : term.cols
    for (let column = start; column < end; column++) {
      const cell = line.getCell(column)
      if (cell && cell.getWidth() !== 0 && /\S/u.test(cell.getChars())) return { x: column, y: row }
    }
  }
  return range.start
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
  let originalSelection: { column: number; rowOffset: number; length: number } | undefined
  const release = () => {
    document.removeEventListener('mousemove', drag, { capture: true })
    document.removeEventListener('mouseup', release)
    originalSelection = undefined
  }
  const drag = () => {
    // Restore the whole-line anchor before xterm handles dragging and scrolling.
    if (originalSelection && service._model.selectionStart) {
      // Use the current row so buffer trimming cannot restore a stale anchor.
      service._model.selectionStart = [originalSelection.column,
        Math.max(0, service._model.selectionStart[1] - originalSelection.rowOffset)]
      service._model.selectionStartLength = originalSelection.length
    }
    release()
  }
  const selectTextLine = (rowIndex: number) => {
    release()
    selectLine.call(service, rowIndex)
    const range = term.getSelectionPosition()
    if (!range) return
    const start = textSelectionStart(term, range)
    originalSelection = {
      column: range.start.x, rowOffset: start.y - range.start.y, length: service._model.selectionStartLength,
    }
    document.addEventListener('mousemove', drag, true)
    document.addEventListener('mouseup', release)
    // Trim before the first native mousedown redraw to avoid highlighting whitespace.
    service._model.selectionStart = [start.x, start.y]
    service._model.selectionStartLength = textSelectionLength(term, { ...range, start })
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
