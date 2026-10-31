/** Fair, bounded terminal parsing on the shared WebView thread. Author: jaswtao · Date: 2026-10-31 */
export const OUTPUT_QUEUE_LIMIT = 256 * 1024
const MAX_QUEUE_AGE = 500
const WRITE_TIMEOUT = 100

export interface SchedulerClock {
  now(): number
  later(action: () => void, delay: number): ReturnType<typeof setTimeout>
  cancel(timer: ReturnType<typeof setTimeout>): void
}

interface OutputItem {
  data: Uint8Array
  offset: number
  added: number
  processed: () => void
}

export class OutputScheduler {
  readonly clock: SchedulerClock
  private ready: OutputQueue[] = []
  private timer: ReturnType<typeof setTimeout> | undefined
  private active: { queue: OutputQueue; timer: ReturnType<typeof setTimeout> } | null = null
  private visibleTurns = 0
  private period = 0
  private spent = 0

  constructor(clock: SchedulerClock = {
    now: () => performance.now(), later: (action, delay) => setTimeout(action, delay), cancel: (timer) => clearTimeout(timer),
  }) { this.clock = clock }

  wake(queue: OutputQueue): void {
    if (!this.ready.includes(queue)) this.ready.push(queue)
    this.schedule()
  }

  cancel(queue: OutputQueue): void {
    this.ready = this.ready.filter((item) => item !== queue)
    if (this.active?.queue === queue) {
      this.clock.cancel(this.active.timer)
      this.active = null
    }
    if (!this.ready.length && this.timer !== undefined) {
      this.clock.cancel(this.timer)
      this.timer = undefined
    }
    this.schedule()
  }

  private schedule(): void {
    if (this.timer !== undefined || this.active || !this.ready.length) return
    const now = this.clock.now()
    // Carry oversized slices into the next interval instead of granting a fresh
    // budget immediately after a slow parser has already consumed the interval.
    const delay = this.spent >= 8 ? Math.max(0, this.period + Math.max(16, this.spent * 2) - now) : 0
    if (!delay && now - this.period >= 16) { this.period = now; this.spent = 0 }
    this.timer = this.clock.later(() => {
      this.timer = undefined
      if (delay > 0) { this.period = this.clock.now(); this.spent = 0 }
      this.pump()
    }, delay)
  }

  private pump(): void {
    this.ready = this.ready.filter((queue) => queue.hasWork)
    if (!this.ready.length) return
    const background = this.ready.some((queue) => !queue.visible())
    const foreground = this.ready.some((queue) => queue.visible())
    const preferVisible = !background || (foreground && this.visibleTurns < 4)
    const index = this.ready.findIndex((queue) => queue.visible() === preferVisible)
    const queue = this.ready.splice(index < 0 ? 0 : index, 1)[0]!
    this.ready.push(queue)
    this.visibleTurns = queue.visible() ? this.visibleTurns + 1 : 0
    const started = this.clock.now()
    const token = { queue, timer: this.clock.later(() => queue.overload(), WRITE_TIMEOUT) }
    this.active = token
    const complete = () => {
      if (this.active !== token) return
      this.clock.cancel(token.timer)
      this.active = null
      this.spent += this.clock.now() - started
      this.schedule()
    }
    try { queue.run(complete) } catch { queue.overload(); complete() }
  }
}

export class OutputQueue {
  private items: OutputItem[] = []
  private bytes = 0
  private chunkSize = 8 * 1024
  private generation = 0
  private maintenance: (() => boolean) | null = null
  private maintenanceTurn = false

  constructor(
    private readonly scheduler: OutputScheduler,
    private readonly write: (data: Uint8Array, complete: () => void) => void,
    readonly visible: () => boolean,
    private readonly onOverload: (discarded: number) => void,
  ) {}

  get hasWork(): boolean { return this.items.length > 0 || this.maintenance !== null }
  get pendingBytes(): number { return this.bytes }

  enqueue(data: Uint8Array, processed: () => void): void {
    if (this.bytes + data.byteLength > OUTPUT_QUEUE_LIMIT) { this.overload(data.byteLength); return }
    this.items.push({ data, offset: 0, added: this.scheduler.clock.now(), processed })
    this.bytes += data.byteLength
    this.scheduler.wake(this)
  }

  scheduleMaintenance(step: () => boolean): void {
    this.maintenance = step
    this.scheduler.wake(this)
  }

  cancelMaintenance(): void { this.maintenance = null }

  clear(): number {
    const discarded = this.bytes
    this.generation++
    this.items = []
    this.bytes = 0
    this.maintenance = null
    this.scheduler.cancel(this)
    return discarded
  }

  overload(incoming = 0): void { this.onOverload(this.clear() + incoming) }

  run(complete: () => void): void {
    if (this.items[0] && this.scheduler.clock.now() - this.items[0].added > MAX_QUEUE_AGE) {
      this.overload(); complete(); return
    }
    if (this.maintenance && (!this.items.length || this.maintenanceTurn)) {
      this.maintenanceTurn = false
      if (this.maintenance()) this.maintenance = null
      complete()
      return
    }
    const item = this.items[0]
    if (!item) { complete(); return }
    this.maintenanceTurn = true
    const generation = this.generation
    const length = Math.min(this.chunkSize, item.data.byteLength - item.offset)
    const started = this.scheduler.clock.now()
    this.write(item.data.subarray(item.offset, item.offset + length), () => {
      if (generation !== this.generation) return
      const elapsed = this.scheduler.clock.now() - started
      if (elapsed > 4) this.chunkSize = Math.max(1024, this.chunkSize >> 1)
      else if (elapsed < 2) this.chunkSize = Math.min(16 * 1024, this.chunkSize * 2)
      item.offset += length
      this.bytes -= length
      if (item.offset === item.data.byteLength) {
        this.items.shift()
        item.processed()
      }
      complete()
    })
  }
}

export const terminalOutputScheduler = new OutputScheduler()
