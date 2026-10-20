/**
 * 终端输入队列：同一时刻只有一个写入在途，等待期间到达的输入合并到下一次发送，保证顺序。
 *
 * Author: jaswtao
 * Date: 2026-10-09
 */

export type InputSender = (data: string) => Promise<void>

export class InputQueue {
  private pending = ''
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
    this.pending += data
    this.pump()
  }

  /** 丢弃尚未发送的输入。 */
  clear(): void {
    this.pending = ''
  }

  private pump(): void {
    if (this.inFlight || !this.sender || !this.pending) return
    const data = this.pending
    const generation = this.generation
    this.pending = ''
    this.inFlight = true
    this.sender(data)
      .catch((error) => { if (generation === this.generation) this.onError(error) })
      .finally(() => {
        if (generation !== this.generation) return
        this.inFlight = false
        this.pump()
      })
  }
}
