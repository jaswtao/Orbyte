/**
 * 终端输入队列：同一时刻只有一个写入在途，等待期间到达的输入合并到下一次发送，保证顺序。
 *
 * Author: jaswtao
 * Date: 2026-10-09
 */

export type InputSender = (data: string) => Promise<void>

export class InputQueue {
  private pending: Array<string | (() => Promise<void>)> = []
  private inFlight = false
  private sender: InputSender | null = null
  private generation = 0
  private paused = false

  constructor(private readonly onError: (error: unknown) => void = () => undefined) {}

  /** 设置发送函数；在终端 ID 就绪之前到达的输入会先排队。 */
  setSender(sender: InputSender | null): void {
    this.generation++
    this.inFlight = false
    this.sender = sender
    this.pump()
  }

  pause(): void { this.paused = true; this.clear(); this.setSender(null) }
  resume(sender: InputSender): void { this.paused = false; this.setSender(sender) }

  push(data: string): void {
    if (!data || this.paused) return
    const last = this.pending.length - 1
    if (typeof this.pending[last] === 'string') this.pending[last] += data
    else this.pending.push(data)
    this.pump()
  }

  /** Queue a backend operation at the same ordering boundary as terminal bytes. */
  pushAction(action: () => Promise<void>): void {
    if (this.paused) return
    this.pending.push(action)
    this.pump()
  }

  /** 丢弃尚未发送的输入。 */
  clear(): void {
    this.pending = []
  }

  private pump(): void {
    if (this.inFlight || !this.sender || !this.pending.length) return
    const data = this.pending.shift()!
    const generation = this.generation
    this.inFlight = true
    const operation = typeof data === 'string' ? this.sender(data) : data()
    operation
      .catch((error) => { if (generation === this.generation) this.onError(error) })
      .finally(() => {
        if (generation !== this.generation) return
        this.inFlight = false
        this.pump()
      })
  }
}
