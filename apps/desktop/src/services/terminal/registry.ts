/** 按配置关联 xterm 与窗格；挂载点可随分屏树移动。Author: jaswtao · Date: 2026-10-13 */
import { markRaw, shallowReactive } from 'vue'
import type { ITerminalOptions } from '@xterm/xterm'
import type { TerminalStatus } from '../terminalApi'
import type { Settings } from '../projectApi'
import { TerminalClient } from './terminalClient'
import { terminalAppearance, type AppearanceMode } from '../appearance'
import { matchShortcut } from '../workspace/shortcuts'
import { t } from '../../i18n/translate'
import { withTimeout } from './recovery'
import { terminalApi } from '../terminalApi'

export interface TerminalEntry {
  client: TerminalClient
  host: HTMLElement
  status: TerminalStatus
  shell: { integrated: boolean; cwd: string | null; command: 'idle' | 'running'; commandStartedAt: number | null; lastExitCode: number | null }
  /** 不可见期间文字内容发生变化；只在状态翻转时写入，避免每帧触发渲染。 */
  unread: boolean
  visible: boolean
  intentionalStop: boolean
  connection: 'connected' | 'reconnecting' | 'disconnected'
}

export class TerminalRegistry {
  constructor(private readonly appearance: () => AppearanceMode, private readonly settings: () => Settings, private readonly shortcutsBlocked: () => boolean = () => false) {}
  readonly byConfig = new Map<string, TerminalEntry>()
  private pollsInFlight = 0
  private pollGeneration = 0

  async syncSessions(): Promise<void> {
    // One in-flight poll may be retried once. Do not queue more until one of them returns.
    if (this.pollsInFlight >= 2 || !this.byConfig.size) return
    this.pollsInFlight++
    const generation = ++this.pollGeneration
    const observed = Array.from(this.byConfig.values(), (entry) => ({ entry, id: entry.client.id }))
    let request: ReturnType<typeof terminalApi.listTerminalSessions>
    try {
      request = terminalApi.listTerminalSessions()
    } catch {
      this.pollsInFlight--
      this.reportPollFailures(generation, observed)
      return
    }
    void request.finally(() => { this.pollsInFlight-- }).catch(() => {})
    try {
      const sessions = await withTimeout(request)
      if (generation !== this.pollGeneration) return
      const current = new Set(this.byConfig.values())
      for (const { entry, id } of observed) {
        if (!id || entry.client.id !== id || !current.has(entry)) continue
        const session = sessions.find((item) => item.terminalId === id)
        if (session) entry.client.observeSession(session)
        else entry.client.updateStatus({ state: 'exited', exitCode: null })
      }
    } catch {
      this.reportPollFailures(generation, observed)
    }
  }

  private reportPollFailures(generation: number, observed: Array<{ entry: TerminalEntry; id: string | null }>): void {
    if (generation !== this.pollGeneration) return
    for (const { entry, id } of observed) {
      if (!id || entry.client.id !== id) continue
      entry.client.reportPollFailed()
    }
  }

  updateSettings(settings: Settings): void {
    for (const entry of this.byConfig.values()) {
      entry.client.term.options.fontFamily = settings.font_family
      entry.client.term.options.fontSize = settings.font_size
      entry.client.term.options.scrollback = settings.scrollback
      entry.client.term.options.theme = terminalAppearance(this.appearance())
      entry.client.refreshSize()
    }
  }

  create(options: ITerminalOptions, onError: (message: string) => void, onCommandEnd?: (duration: number, exitCode?: number) => void, onExit?: (exitCode: number | null) => void, onWarning?: (message: string) => void): TerminalEntry {
    const host = document.createElement('div')
    host.className = 'terminal-host'
    const client = markRaw(new TerminalClient(host, { ...options, theme: terminalAppearance(this.appearance()) }, {
      onConnection: (state) => { entry.connection = state },
      onWarning,
      onStatus: (status) => {
        const exitedAlready = entry.status.state === 'exited'
        entry.status = status
        if (status.state === 'exited' && !exitedAlready && !entry.intentionalStop) onExit?.(status.exitCode)
      },
      onOutput: () => { if (!entry.visible && !entry.unread) entry.unread = true },
      onShell: (event) => {
        const shell = { ...entry.shell, integrated: true }
        if (event.type === 'cwd') shell.cwd = event.path
        if (event.type === 'commandStart') { shell.command = 'running'; shell.commandStartedAt = Date.now(); shell.lastExitCode = null }
        if (event.type === 'commandEnd') {
          const duration = shell.commandStartedAt == null ? 0 : Date.now() - shell.commandStartedAt
          shell.command = 'idle'; shell.commandStartedAt = null; shell.lastExitCode = event.exitCode ?? null
          onCommandEnd?.(duration, event.exitCode)
        }
        entry.shell = shell
      },
      onError,
      onGap: (bytes) => { onWarning?.(t('terminal.outputGap', { bytes: bytes.toLocaleString() })) },
    }))
    const entry = shallowReactive<TerminalEntry>({ host: markRaw(host), client, status: { state: 'starting' }, shell: { integrated: false, cwd: null, command: 'idle', commandStartedAt: null, lastExitCode: null }, unread: false, visible: false, intentionalStop: false, connection: 'connected' })
    entry.client.term.attachCustomKeyEventHandler((event) => {
      // Windows 输入法会把 Esc 标记为 229，xterm 6 会在编码前丢弃它。
      // 正在组词时仍交给输入法取消候选，避免误中断终端程序。
      if (event.keyCode === 229 && event.code === 'Escape' && !event.isComposing &&
        !event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey) {
        if (event.type === 'keydown') {
          event.preventDefault()
          event.stopPropagation()
          entry.client.term.input('\x1b', true)
        }
        return false
      }
      if (this.shortcutsBlocked()) return true
      const command = matchShortcut(event, this.settings().shortcuts, ['project', 'workspace', 'terminal'])
      if (!command) return true
      if (event.type === 'keydown') {
        event.preventDefault()
        if (command === 'terminalCopy') void copy()
        if (command === 'terminalPaste') void paste()
        if (command === 'terminalPasteAuto') void paste(true)
      }
      return false
    })
    const copy = async () => {
      try { if (entry.client.term.hasSelection()) await terminalApi.writeClipboardText(entry.client.term.getSelection()) }
      catch (error) { onError(t('terminal.copyFailed', { reason: String(error) })) }
    }
    const paste = async (forwardWithoutText = false) => {
      try {
        const text = await terminalApi.readClipboardText()
        if (text) entry.client.term.paste(text)
        else if (forwardWithoutText) {
          // Native clipboard access also works when WebView denies browser clipboard reads.
          const path = await terminalApi.saveClipboardImage()
          if (!path) return
          entry.client.term.paste(`"${path.replace(/\\/g, '/')}" `)
        }
      }
      catch (error) { onError(t('terminal.pasteFailed', { reason: String(error) })) }
    }
    // 右键由宿主统一复制/粘贴，避免 CLI 同时收到鼠标上报并再次处理剪贴板。
    const interceptRightButton = (event: MouseEvent) => {
      if (event.button !== 2) return
      event.preventDefault()
      event.stopPropagation()
    }
    host.addEventListener('mousedown', interceptRightButton, true)
    host.addEventListener('mouseup', interceptRightButton, true)
    host.addEventListener('contextmenu', (event) => {
      event.preventDefault()
      event.stopPropagation()
      const term = entry.client.term
      if (term.hasSelection()) {
        void copy()
        term.clearSelection()
      } else {
        void paste()
      }
      term.focus()
    }, true)
    return entry
  }

  bind(configId: string, entry: TerminalEntry, sessionId: string): void {
    this.byConfig.set(configId, entry)
    entry.client.attach(sessionId)
  }

  remove(configId: string): void {
    const entry = this.byConfig.get(configId)
    if (!entry) return
    entry.client.dispose()
    entry.host.remove()
    this.byConfig.delete(configId)
  }
}
