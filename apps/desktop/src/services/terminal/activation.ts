/** 通知宿主当前交互的终端；操作其他应用时保留会话关联。Author: jaswtao · Date: 2026-10-16 */
export function bindTerminalActivation(document: Document, send: (id: string | null) => Promise<void>): () => void {
  let disposed = false
  let pending = Promise.resolve()
  let activeId: string | null = null
  const select = (id: string | null) => {
    activeId = id
    // 串行提交，避免快速切换时 IPC 完成顺序覆盖最后一次选择。
    pending = pending.then(async () => { if (!disposed) await send(id) }).catch(console.warn)
  }
  const sessionId = (event: Event) => {
    const target = event.target as Element | null
    return target?.closest?.<HTMLElement>('[data-terminal-session]')?.dataset.terminalSession ?? null
  }
  const update = (event: Event) => {
    select(sessionId(event))
  }
  const hidden = (event: Event) => {
    if (activeId !== null && sessionId(event) === activeId) select(null)
  }
  document.addEventListener('focusin', update)
  document.addEventListener('pointerdown', update, true)
  document.addEventListener('terminal-session-attached', update)
  document.addEventListener('terminal-session-hidden', hidden)
  return () => {
    disposed = true
    document.removeEventListener('focusin', update)
    document.removeEventListener('pointerdown', update, true)
    document.removeEventListener('terminal-session-attached', update)
    document.removeEventListener('terminal-session-hidden', hidden)
    void send(null).catch(console.warn)
  }
}
