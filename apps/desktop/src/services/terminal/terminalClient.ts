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

import { terminalApi, type TerminalSize, type TerminalStatus } from '../terminalApi'
import { decodeFrame, decodeJsonPayload, FrameKind } from './frame'
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
  private unackedBytes = 0
  private ackTimer: number | undefined
  private resizeTimer: number | undefined
  private contentTimer: number | undefined
  private readonly contentActivity = new ContentActivity()
  private lastSize: TerminalSize | null = null
  private readonly observer: ResizeObserver
  private readonly ime: ReturnType<typeof bindTerminalIme>
  private readonly selection: IDisposable
  private pendingChannel: { frames: ArrayBuffer[] } | null = null
  private activeChannel: { frames: ArrayBuffer[] } | null = null
  status: TerminalStatus = { state: 'starting' }

  constructor(
    private readonly container: HTMLElement,
    options: ITerminalOptions,
    private readonly events: TerminalClientEvents = {},
  ) {
    this.term = new Terminal({ scrollback: 5000, allowProposedApi: false, wordSeparator: ' ()[]{}\',"`\\/:>-', ...options })
    this.term.loadAddon(this.fit)
    this.term.open(container)
    this.term.onResize(() => this.contentActivity.resized(this.term.buffer.active))
    this.term.onWriteParsed(() => {
      // 合并清屏与重画；持续输出时最多每 200ms 比较一次。
      if (this.contentTimer === undefined) this.contentTimer = window.setTimeout(() => this.flushContentActivity(), CONTENT_CHECK_MS)
    })
    this.ime = bindTerminalIme(this.term)
    this.selection = bindTerminalSelection(this.term)
    for (const code of [133, 9, 7]) this.term.parser.registerOscHandler(code, (data) => {
      if (code === 133 && data === 'C') this.contentActivity.commandStart(this.term)
      return this.integration.osc(code, data)
    })
    this.term.onData((data) => { this.integration.input(data); this.input.push(data) })
    this.observer = new ResizeObserver(() => this.scheduleResize())
    this.observer.observe(container)
  }

  get id(): string | null {
    return this.terminalId
  }

  /** 创建绑定到本实例的 Channel，用于 create / start / subscribe 命令。 */
  createChannel(): Channel<ArrayBuffer> {
    const subscription = { frames: [] as ArrayBuffer[] }
    this.pendingChannel = subscription
    const channel = new Channel<ArrayBuffer>()
    channel.onmessage = (buffer) => {
      if (this.pendingChannel === subscription) subscription.frames.push(buffer)
      else if (this.activeChannel === subscription) this.handleFrame(buffer)
    }
    return channel
  }

  /** 命令返回 terminal_id 后绑定，之前排队的输入和 ack 随之发出。 */
  attach(terminalId: string): void {
    this.integration.reset()
    this.contentActivity.reset(this.term.buffer.active)
    window.clearTimeout(this.ackTimer)
    this.ackTimer = undefined
    this.processedSeq = null
    this.unackedBytes = 0
    this.input.setSender(null)
    if (this.terminalId !== null) this.input.clear()
    this.terminalId = terminalId
    this.container.dataset.terminalSession = terminalId
    if (this.container.contains(document.activeElement)) this.container.dispatchEvent(new Event('terminal-session-attached', { bubbles: true }))
    this.lastSize = null
    this.input.setSender((data) => terminalApi.writeTerminal(terminalId, data))
    const subscription = this.pendingChannel
    if (subscription) {
      this.activeChannel = subscription
      this.pendingChannel = null
      for (const frame of subscription.frames) this.handleFrame(frame)
      subscription.frames = []
    }
    this.flushAck()
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
    return this.container.offsetWidth > 0 && this.container.offsetHeight > 0
  }

  private handleFrame(buffer: ArrayBuffer): void {
    const frame = decodeFrame(buffer)
    switch (frame.kind) {
      case FrameKind.Output: {
        const length = frame.payload.byteLength
        const subscription = this.activeChannel
        this.term.write(frame.payload, () => {
          if (this.activeChannel === subscription) { this.onProcessed(frame.seq, length) }
        })
        break
      }
      case FrameKind.Status:
        this.status = decodeJsonPayload<TerminalStatus>(frame.payload)
        this.events.onStatus?.(this.status)
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
    this.processedSeq = seq
    this.unackedBytes += length
    if (this.unackedBytes >= ACK_BYTES) {
      this.flushAck()
    } else if (this.ackTimer === undefined) {
      this.ackTimer = window.setTimeout(() => this.flushAck(), ACK_IDLE_MS)
    }
  }

  private flushAck(): void {
    window.clearTimeout(this.ackTimer)
    this.ackTimer = undefined
    if (this.terminalId === null || this.processedSeq === null || this.unackedBytes === 0) return
    this.unackedBytes = 0
    terminalApi.ackTerminal(this.terminalId, this.processedSeq).catch(() => undefined)
  }

  private scheduleResize(): void {
    window.clearTimeout(this.resizeTimer)
    this.resizeTimer = window.setTimeout(() => this.applyResize(), RESIZE_DEBOUNCE_MS)
  }

  private applyResize(): void {
    const size = this.measure()
    if (!size) return
    this.fit.fit()
    if (this.terminalId === null) return
    if (this.lastSize && this.lastSize.cols === size.cols && this.lastSize.rows === size.rows) return
    this.lastSize = size
    terminalApi.resizeTerminal(this.terminalId, size).catch(() => undefined)
  }
}
