/** Serialized recovery writes with bounded debounce. Author: jaswtao · Date: 2026-10-24 */
export interface DraftSnapshot { revision: string; content: string; updatedAt?: string }
export class DraftSync<T extends DraftSnapshot> {
  private queue: Promise<void> = Promise.resolve()
  private timer: ReturnType<typeof setTimeout> | undefined
  private deadline: ReturnType<typeof setTimeout> | undefined
  private persisted: T | null
  private stopped = false
  private discarded = false
  private discardPending = false
  constructor(private snapshot: () => T | null, private put: (draft: T) => Promise<void>, private remove: (draft: T) => Promise<void>, private failed: (reason: unknown) => void, initial: T | null = null, private debounce = 750, private maximum = 5000) { this.persisted = initial }
  schedule() {
    if (this.stopped) return
    this.discarded = false
    clearTimeout(this.timer)
    this.timer = setTimeout(() => { void this.flush().catch(() => {}) }, this.debounce)
    this.deadline ??= setTimeout(() => { void this.flush().catch(() => {}) }, this.maximum)
  }
  private cancelTimers() { clearTimeout(this.timer); clearTimeout(this.deadline); this.timer = undefined; this.deadline = undefined }
  private enqueue(operation: () => Promise<void>) {
    const next = this.queue.catch(() => {}).then(operation)
    this.queue = next
    void next.catch(this.failed)
    return next
  }
  flush() {
    this.cancelTimers()
    if (this.stopped) return this.queue
    if (this.discarded) return this.discardPending ? this.discard() : this.queue
    const draft = this.snapshot()
    return this.enqueue(async () => {
      if (!draft) {
        if (this.persisted) { await this.remove(this.persisted); this.persisted = null }
      } else {
        const { revision: _a, updatedAt: _timeA, ...next } = draft
        const { revision: _b, updatedAt: _timeB, ...previous } = this.persisted ?? {} as T
        if (JSON.stringify(next) === JSON.stringify(previous)) return
        await this.put(draft); this.persisted = draft
      }
    })
  }
  discard() {
    this.cancelTimers()
    this.stopped = true
    this.discarded = true
    this.discardPending = true
    return this.enqueue(async () => {
      if (this.persisted) { await this.remove(this.persisted); this.persisted = null }
      this.discardPending = false
    }).finally(() => { this.stopped = false })
  }
  deleteDraft(draft: T) {
    return this.enqueue(async () => {
      await this.remove(draft)
      if (this.persisted?.revision === draft.revision) this.persisted = null
    })
  }
  async pause() { this.cancelTimers(); this.stopped = true; await this.queue.catch(() => {}) }
  relocate(path: string) { if (this.persisted) this.persisted = { ...this.persisted, path } }
  resume(initial?: T | null) { if (initial !== undefined) this.persisted = initial; this.stopped = false }
  dispose() { this.cancelTimers(); this.stopped = true }
}
