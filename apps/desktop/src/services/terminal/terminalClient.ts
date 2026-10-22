/**
 * 单个终端的前端实例：xterm、Channel 帧处理、ack、输入队列和尺寸同步。
 * 项目切换时只隐藏容器，不销毁实例。
 *
 * Author: jaswtao
 * Date: 2026-10-09
 */
import { Channel } from '@tauri-apps/api/core'
import { FitAddon } from '@xterm/addon-fit'
import { Terminal, type IDisposable, type ITerminalOptions } from '@xterm/xterm'

import { terminalApi, type TerminalSize, type TerminalStatus, type TerminalSessionInfo } from '../terminalApi'
import { AckQueue, OutputRecovery, withTimeout } from './recovery'
import { decodeFrame, decodeJsonPayload, FrameKind, type TerminalFrameData } from './frame'
import { InputQueue } from './inputQueue'
import { bindTerminalIme } from './ime'
import { bindTerminalSelection } from './selection'
import { ShellIntegration, type ShellEvent } from './shellIntegration'
import { ContentActivity } from './contentActivity'
import { t } from '../../i18n/translate'

const ACK_BYTES = 256 * 1024
const ACK_IDLE_MS = 50
const RESIZE_DEBOUNCE_MS = 100
const CONTENT_CHECK_MS = 200
export const INITIAL_SIZE: TerminalSize = { cols: 120, rows: 30 }

export interface TerminalClientEvents {
  onOutput?: () => void
  onShell?: (event: ShellEvent) => void
  onStatus?: (status: TerminalStatus) => void
  onError?: (message: string) => void
  onGap?: (droppedBytes: number) => void
  onConnection?: (state: 'connected' | 'reconnecting' | 'disconnected') => void
  onWarning?: (message: string) => void
}

export class TerminalClient {
  readonly term: Terminal
  readonly integration = new ShellIntegration((event) => {
    if (event.type === 'promptStart') {
      this.flushContentActivity()
      this.contentActivity.promptStart(this.term)
    }
    if (event.type === 'promptEnd') this.contentActivity.promptEnd(this.term)
    this.events.onShell?.(event)
  })
  private readonly fit = new FitAddon()
  private readonly input = new InputQueue((error) => this.events.onError?.(t('terminal.inputFailed', { reason: String(error) })))
  private terminalId: string | null = null
  private processedSeq: number | null = null
  private readonly ack = new AckQueue((seq) => this.terminalId
    ? withTimeout(terminalApi.ackTerminal(this.terminalId, seq)) : Promise.resolve())
  private readonly recovery = new OutputRecovery()
  private generation = 0
  private ackFailures = 0
  private disposed = false
  private operationPaused = false
  private awaitingStatus = false
  private inputEnabled = false
  private reconnecting = false
  private recoveryAttempt = 0
  private recoveredOutput: (() => void) | null = null
  private connection: 'connected' | 'reconnecting' | 'disconnected' = 'connected'
  private ackTimer: number | undefined
  private resizeTimer: number | undefined
  private contentTimer: number | undefined
  private readonly contentActivity = new ContentActivity()
  private lastSize: TerminalSize | null = null
  private readonly observer: ResizeObserver
  private readonly ime: ReturnType<typeof bindTerminalIme>
  private readonly selection: IDisposable
  private pendingChannel: { frames: TerminalFrameData[] } | null = null
  private activeChannel: { frames: TerminalFrameData[] } | null = null
  status: TerminalStatus = { state: 'starting' }

  constructor(
    private readonly container: HTMLElement,
    options: ITerminalOptions,
    private readonly events: TerminalClientEvents = {},
  ) {
    this.term = new Terminal({ scrollback: 5000, allowProposedApi: false, wordSeparator: ' ()[]{}\',"`\\/:>-', ...options })
    this.term.loadAddon(this.fit)
    this.term.open(container)
    this.term.onResize(() => this.safe(() => this.contentActivity.resized(this.term.buffer.active)))
    this.term.onWriteParsed(() => {
      // 合并清屏与重画；持续输出时最多每 200ms 比较一次。
      if (this.contentTimer === undefined) this.contentTimer = window.setTimeout(() => this.safe(() => this.flushContentActivity()), CONTENT_CHECK_MS)
    })
    this.ime = bindTerminalIme(this.term)
    this.selection = bindTerminalSelection(this.term)
    for (const code of [133, 9, 7]) this.term.parser.registerOscHandler(code, (data) => {
      this.safe(() => {
        if (code === 133 && data === 'C') this.contentActivity.commandStart(this.term)
        this.integration.osc(code, data)
      })
      return true
    })
    this.term.onData((data) => this.safe(() => { if (!this.term.options.disableStdin) { this.integration.input(data); this.input.push(data) } }))
    this.observer = new ResizeObserver(() => this.scheduleResize())
    this.observer.observe(container)
  }

  get id(): string | null {
    return this.terminalId
  }

  /** 创建绑定到本实例的 Channel，用于 create / start / subscribe 命令。 */
  createChannel(): Channel<TerminalFrameData> {
    const subscription = { frames: [] as TerminalFrameData[] }
    this.pendingChannel = subscription
    const channel = new Channel<TerminalFrameData>()
    channel.onmessage = (buffer) => {
      this.safe(() => {
        if (this.pendingChannel === subscription) subscription.frames.push(buffer)
        else if (this.activeChannel === subscription) this.handleFrame(buffer)
      })
    }
    return channel
  }

  /** 命令返回 terminal_id 后绑定，之前排队的输入和 ack 随之发出。 */
  attach(terminalId: string): void {
    if (this.terminalId !== terminalId) {
      this.recovery.reset()
      this.status = { state: 'starting' }
      this.operationPaused = false
      this.awaitingStatus = false
    }
    this.generation++
    this.ack.reset()
    this.ackFailures = 0
    this.integration.reset()
    this.contentActivity.reset(this.term.buffer.active)
    window.clearTimeout(this.ackTimer)
    this.ackTimer = undefined
    this.processedSeq = null
    this.input.setSender(null)
    this.inputEnabled = false
    this.input.pause()
    if (this.terminalId !== null) this.input.clear()
    this.terminalId = terminalId
    this.container.dataset.terminalSession = terminalId
    if (this.container.contains(document.activeElement)) this.container.dispatchEvent(new Event('terminal-session-attached', { bubbles: true }))
    this.lastSize = null
    const subscription = this.pendingChannel
    if (subscription) {
      this.activeChannel = subscription
      this.pendingChannel = null
      for (const frame of subscription.frames) this.safe(() => this.handleFrame(frame))
      subscription.frames = []
    }
    this.syncInput()
    void this.flushAck()
    this.scheduleResize()
  }

  /** 当前容器可见时的行列数；不可见返回 null。 */
  measure(): TerminalSize | null {
    if (!this.isVisible()) return null
    const dims = this.fit.proposeDimensions()
    if (!dims || !dims.cols || !dims.rows) return null
    return { cols: dims.cols, rows: dims.rows }
  }

  /** 容器重新显示后调用，重新计算一次行列数。 */
  refreshSize(): void {
    this.scheduleResize()
  }

  focus(): void {
    this.term.focus()
  }

  reset(): void {
    this.term.reset()
    this.ime.reset()
  }

  /** 可见性切换前结算内容变化，避免已看过的输出延迟触发未读。 */
  flushContentActivity(): void {
    window.clearTimeout(this.contentTimer)
    this.contentTimer = undefined
    if (this.contentActivity.changed(this.term.buffer.active)) this.events.onOutput?.()
  }

  dispose(): void {
    this.disposed = true
    this.generation++
    this.ack.reset()
    this.pauseInput()
    this.contentActivity.reset()
    delete this.container.dataset.terminalSession
    this.activeChannel = null
    this.pendingChannel = null
    this.observer.disconnect()
    window.clearTimeout(this.ackTimer)
    window.clearTimeout(this.resizeTimer)
    window.clearTimeout(this.contentTimer)
    this.input.setSender(null)
    this.ime.dispose()
    this.selection.dispose()
    this.term.dispose()
  }

  private isVisible(): boolean {
    return document.visibilityState !== 'hidden' && this.container.offsetWidth > 0 && this.container.offsetHeight > 0
  }

  pauseInput(): void {
    this.awaitingStatus = false
    this.recoveryAttempt++
    this.generation++
    this.pendingChannel = null
    this.operationPaused = true
    this.syncInput()
  }

  resumeInput(): void {
    this.operationPaused = false
    this.syncInput()
  }

  waitForStatus(): void { this.awaitingStatus = true }

  updateStatus(status: TerminalStatus): void {
    if (!status || !['starting', 'running', 'exited', 'failed'].includes(status.state)) throw new Error('Invalid terminal status')
    if (this.disposed || (this.status.state === 'exited' && status.state === 'running')) return
    const changed = JSON.stringify(status) !== JSON.stringify(this.status)
    this.status = status
    if (status.state === 'exited' || status.state === 'failed') {
      this.pendingChannel = null
      // An independent status poll can finish before the last output. Still accept and acknowledge trailing frames.
      this.setConnection('connected')
    }
    this.syncInput()
    if (changed) this.safe(() => this.events.onStatus?.(status))
  }

  observeSession(session: TerminalSessionInfo): void {
    if (this.disposed || this.terminalId !== session.terminalId) return
    this.updateStatus(session.status)
    if (this.awaitingStatus) { this.awaitingStatus = false; this.resumeInput() }
    if (session.status.state === 'running' && !this.operationPaused && !this.reconnecting) {
      const recover = this.recovery.observe(session.output, this.isVisible(), performance.now())
      if (this.recovery.unhealthy) { this.setConnection('disconnected'); this.syncInput() }
      if (recover) void this.reconnect()
    }
  }

  async reconnect(): Promise<void> {
    const terminalId = this.terminalId
    if (!terminalId || this.disposed || this.reconnecting || this.operationPaused || this.status.state !== 'running') return
    this.reconnecting = true
    const attempt = ++this.recoveryAttempt
    const cancelled = () => this.disposed || this.operationPaused || this.terminalId !== terminalId || this.recoveryAttempt !== attempt
    this.activeChannel = null
    this.ack.reset()
    this.setConnection('reconnecting')
    this.syncInput()
    this.diagnostic('reconnect_started')
    try {
      const sessions = await withTimeout(terminalApi.listTerminalSessions())
      if (cancelled()) return
      const session = sessions.find((item) => item.terminalId === terminalId)
      if (!session) throw new Error('Terminal session missing')
      this.updateStatus(session.status)
      if (session.status.state !== 'running') return
      const parsed = new Promise<void>((resolve) => { this.recoveredOutput = resolve })
      await withTimeout(terminalApi.subscribeTerminal(terminalId, this.createChannel()))
      if (cancelled() || this.status.state !== 'running') return
      this.attach(terminalId)
      const size = this.measure()
      if (size) {
        // The nudge and restore finish inside one backend call, so a late call after timeout cannot stay one column off.
        await withTimeout(terminalApi.resizeTerminal(terminalId, size, true))
        if (cancelled() || this.status.state !== 'running') return
      }
      await withTimeout(parsed)
      if (cancelled() || this.status.state !== 'running') return
      this.setConnection('connected')
      this.recovery.restored()
      this.diagnostic('reconnect_succeeded')
      this.safe(() => this.events.onWarning?.(t('terminal.reconnected')))
    } catch {
      if (!cancelled() && this.status.state === 'running') {
        this.pendingChannel = null
        this.setConnection('disconnected')
        this.diagnostic('reconnect_failed')
        this.safe(() => this.events.onWarning?.(t('terminal.reconnectFailed')))
      }
    } finally {
      this.recoveredOutput = null
      if (this.connection === 'reconnecting' && !this.disposed) {
        this.pendingChannel = null
        this.setConnection(this.status.state === 'running' ? 'disconnected' : 'connected')
      }
      this.reconnecting = false
      this.syncInput()
    }
  }

  reportPollFailed(): void {
    this.diagnostic('poll_failed')
  }

  private syncInput(): void {
    const enabled = !this.disposed && !this.operationPaused && !this.reconnecting && this.connection === 'connected' && this.status.state === 'running'
    this.term.options.disableStdin = !enabled
    if (enabled === this.inputEnabled) return
    this.inputEnabled = enabled
    if (enabled && this.terminalId) {
      const id = this.terminalId
      this.input.resume((data) => terminalApi.writeTerminal(id, data))
    } else this.input.pause()
  }

  private setConnection(state: 'connected' | 'reconnecting' | 'disconnected'): void {
    if (this.connection === state) return
    this.connection = state
    try { this.events.onConnection?.(state) } catch { /* Connection notifications must not block recovery. */ }
  }

  private diagnostic(event: Parameters<typeof terminalApi.reportDiagnostic>[1]): void {
    try {
      if (this.terminalId) void terminalApi.reportDiagnostic(this.terminalId, event, this.generation, this.processedSeq).catch(() => {})
    } catch { /* Diagnostic failures must not block ordered message handling. */ }
  }

  private safe(action: () => void): void {
    if (this.disposed) return
    try { action() } catch {
      this.diagnostic('frame_error')
      if (this.status.state === 'exited' || this.status.state === 'failed') return
      this.setConnection('disconnected')
      this.syncInput()
      if (this.recovery.fault(this.isVisible())) queueMicrotask(() => { void this.reconnect() })
    }
  }

  private handleFrame(buffer: TerminalFrameData): void {
    const frame = decodeFrame(buffer)
    switch (frame.kind) {
      case FrameKind.Output: {
        const length = frame.payload.byteLength
        const subscription = this.activeChannel
        this.term.write(frame.payload, () => {
          this.safe(() => { if (this.activeChannel === subscription) this.onProcessed(frame.seq, length) })
        })
        break
      }
      case FrameKind.Status:
        this.updateStatus(decodeJsonPayload<TerminalStatus>(frame.payload))
        break
      case FrameKind.Error:
        this.events.onError?.(decodeJsonPayload<{ message: string }>(frame.payload).message)
        break
      case FrameKind.Gap:
        this.events.onGap?.(decodeJsonPayload<{ droppedBytes: number }>(frame.payload).droppedBytes)
        break
    }
  }

  private onProcessed(seq: number, length: number): void {
    this.recoveredOutput?.()
    this.recoveredOutput = null
    this.processedSeq = seq
    this.ack.processed(seq, length)
    if (this.ack.bytes >= ACK_BYTES) {
      void this.flushAck()
    } else if (this.ackTimer === undefined) {
      this.ackTimer = window.setTimeout(() => { void this.flushAck() }, ACK_IDLE_MS)
    }
  }

  private async flushAck(): Promise<void> {
    window.clearTimeout(this.ackTimer)
    this.ackTimer = undefined
    if (!this.terminalId || !this.ack.bytes || this.disposed || this.ack.busy) return
    const generation = this.generation
    const success = await this.ack.flush()
    if (generation !== this.generation || this.disposed) return
    if (success) this.ackFailures = 0
    else {
      this.ackFailures++
      if (this.ackFailures === 1) this.diagnostic('ack_failed')
      if (this.ackFailures >= 3 && this.status.state === 'running') {
        this.setConnection('disconnected')
        this.syncInput()
        if (this.recovery.fault(this.isVisible())) void this.reconnect()
      }
    }
    if (this.ack.bytes && this.ackFailures < 3 && this.ackTimer === undefined) this.ackTimer = window.setTimeout(() => { void this.flushAck() }, success ? ACK_IDLE_MS : 500)
  }

  private scheduleResize(): void {
    window.clearTimeout(this.resizeTimer)
    this.resizeTimer = window.setTimeout(() => this.applyResize(), RESIZE_DEBOUNCE_MS)
  }

  private applyResize(): void {
    if (this.reconnecting || this.disposed) return
    const size = this.measure()
    if (!size) return
    this.fit.fit()
    if (this.terminalId === null) return
    if (this.lastSize && this.lastSize.cols === size.cols && this.lastSize.rows === size.rows) return
    this.lastSize = size
    terminalApi.resizeTerminal(this.terminalId, size).catch(() => undefined)
  }
}
