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

  constructor(private readonly onError: (error: unknown) => void = () => undefined) {}

  /** 设置发送函数；在终端 ID 就绪之前到达的输入会先排队。 */
  setSender(sender: InputSender | null): void {
    this.sender = sender
    this.pump()
  }

  push(data: string): void {
    if (!data) return
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
    this.pending = ''
    this.inFlight = true
    this.sender(data)
      .catch((error) => this.onError(error))
      .finally(() => {
        this.inFlight = false
        this.pump()
      })
  }
}
