/** Output acknowledgement and one-shot automatic recovery. Author: jaswtao · Date: 2026-10-20 */
export interface OutputHealth {
  paused: boolean
  droppedBytes: number
  subscribed: boolean
  inflightBytes: number
  ackIdleMs: number | null
}

export class AckQueue {
  bytes = 0
  private seq: number | null = null
  private sending = false
  private generation = 0

  constructor(private readonly send: (seq: number) => Promise<void>) {}
  get busy(): boolean { return this.sending }

  processed(seq: number, bytes: number): void {
    this.seq = this.seq === null ? seq : Math.max(this.seq, seq)
    this.bytes += bytes
  }

  reset(): void {
    this.generation++
    this.bytes = 0
    this.seq = null
    this.sending = false
  }

  async flush(): Promise<boolean> {
    if (this.sending || this.seq === null || !this.bytes) return true
    const generation = this.generation
    const bytes = this.bytes
    this.sending = true
    try {
      await this.send(this.seq)
      if (generation === this.generation) this.bytes -= bytes
      return true
    } catch {
      return false
    } finally {
      if (generation === this.generation) this.sending = false
    }
  }
}

export class OutputRecovery {
  unhealthy = false
  private used = false
  private broken = false
  private stalledSince: number | null = null

  reset(): void { this.used = false; this.restored() }
  restored(): void { this.broken = false; this.unhealthy = false; this.stalledSince = null }

  fault(visible: boolean): boolean {
    this.broken = true
    this.unhealthy = true
    return visible && this.claim()
  }

  observe(health: OutputHealth, visible: boolean, now: number): boolean {
    if (!visible) { this.stalledSince = null; return false }
    if (this.broken || !health.subscribed) { this.unhealthy = true; return this.claim() }
    if (!health.inflightBytes) {
      this.stalledSince = null
      this.unhealthy = false
      return false
    }
    this.stalledSince ??= now
    this.unhealthy = (health.ackIdleMs ?? 0) >= 10000 && now - this.stalledSince >= 10000
    return this.unhealthy && this.claim()
  }

  private claim(): boolean {
    if (this.used) return false
    this.used = true
    return true
  }
}

/** An RPC timeout only stops waiting. A late result must not be applied to the current subscription. */
export async function withTimeout<T>(operation: Promise<T>, milliseconds = 5000): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    return await Promise.race([operation, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error('terminal RPC timeout')), milliseconds)
    })])
  } finally { clearTimeout(timer) }
}
