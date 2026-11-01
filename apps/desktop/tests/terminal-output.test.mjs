/** Terminal load isolation regressions. Author: jaswtao · Date: 2026-10-31 */
import assert from 'node:assert/strict'
import test from 'node:test'
import xterm from '@xterm/xterm'
import { loadService } from './load-service.mjs'

const { OutputQueue, OutputScheduler, OUTPUT_QUEUE_LIMIT } = await loadService('terminal/outputScheduler')
const { ContentActivity } = await loadService('terminal/contentActivity')

function fixture() {
  let now = 0
  let id = 0
  const timers = new Map()
  const clock = {
    now: () => now,
    later(action, delay) { const token = ++id; timers.set(token, { action, at: now + delay }); return token },
    cancel(token) { timers.delete(token) },
  }
  return { scheduler: new OutputScheduler(clock), timers,
    advance(milliseconds) {
      const end = now + milliseconds
      let turns = 0
      while (true) {
        const next = [...timers].sort((a, b) => a[1].at - b[1].at)[0]
        if (!next || next[1].at > end) break
        assert.ok(++turns < 10000, 'Scheduler did not yield')
        now = next[1].at
        timers.delete(next[0])
        next[1].action()
      }
      now = Math.max(now, end)
    },
    cost(milliseconds) { now += milliseconds },
  }
}

test('output is split, acknowledged once per frame, and rotated between terminals', () => {
  const f = fixture()
  const turns = []
  const complete = []
  const queues = ['a', 'b', 'c'].map((name) => new OutputQueue(f.scheduler,
    (data, done) => { turns.push([name, data.length]); f.cost(3); done() }, () => true, () => assert.fail('unexpected overload')))
  queues.forEach((queue, index) => queue.enqueue(new Uint8Array(64 * 1024), () => complete.push(index)))
  assert.equal(turns.length, 0, 'Channel delivery must not parse synchronously')
  f.advance(200)
  assert.deepEqual(turns.slice(0, 3).map(([name]) => name), ['a', 'b', 'c'])
  assert.deepEqual(complete.sort(), [0, 1, 2])
  assert.ok(turns.every(([, bytes]) => bytes <= 16 * 1024))
  assert.ok(queues.every((queue) => queue.pendingBytes === 0))
})

test('global parsing budget leaves time for application input between output batches', () => {
  const f = fixture()
  const events = []
  const queue = new OutputQueue(f.scheduler, (_, done) => { events.push('parse'); f.cost(3); done() }, () => true, () => {})
  queue.enqueue(new Uint8Array(128 * 1024), () => {})
  f.scheduler.clock.later(() => events.push('application input'), 10)
  f.advance(200)
  assert.equal(events.indexOf('application input'), 3)
})

test('a slow parsing slice must repay its budget before parsing the next chunk', () => {
  const f = fixture()
  let cost = 0
  const queue = new OutputQueue(f.scheduler, (_, done) => { cost += 17; f.cost(17); done() }, () => true, () => {})
  queue.enqueue(new Uint8Array(128 * 1024), () => {})
  f.advance(200)
  assert.ok(cost <= 117, `Slow parsing monopolized the event loop for ${cost} ms`)
  queue.clear()
})

test('a busy visible terminal cannot starve hidden terminals', () => {
  const f = fixture()
  const turns = []
  const visible = new OutputQueue(f.scheduler, (_, done) => { turns.push('visible'); done() }, () => true, () => {})
  const hidden = new OutputQueue(f.scheduler, (_, done) => { turns.push('hidden'); done() }, () => false, () => {})
  visible.enqueue(new Uint8Array(128 * 1024), () => {})
  hidden.enqueue(new Uint8Array(128 * 1024), () => {})
  f.advance(0)
  assert.deepEqual(turns.slice(0, 5), ['visible', 'visible', 'visible', 'visible', 'hidden'])
  assert.equal(hidden.pendingBytes, 0)
})

test('a missing write callback only suspends its own terminal and ignores late completion', () => {
  const f = fixture()
  const paused = []
  let late
  let ack = 0
  const broken = new OutputQueue(f.scheduler, (_, done) => { late = done }, () => true, (bytes, reason) => paused.push([bytes, reason]))
  const normal = new OutputQueue(f.scheduler, (_, done) => done(), () => true, () => assert.fail('normal terminal paused'))
  broken.enqueue(new Uint8Array(64), () => ack++)
  normal.enqueue(new Uint8Array(64), () => ack++)
  f.advance(101)
  assert.deepEqual(paused, [[64, 'timeout']])
  assert.equal(ack, 1)
  late()
  assert.equal(ack, 1)
  assert.equal(f.timers.size, 0)
})

test('queue overflow discards locally, without accepting unbounded data', () => {
  const f = fixture()
  const discarded = []
  const queue = new OutputQueue(f.scheduler, () => assert.fail('unexpected write'), () => true, (bytes, reason) => discarded.push([bytes, reason]))
  queue.enqueue(new Uint8Array(OUTPUT_QUEUE_LIMIT), () => assert.fail('discarded frame acknowledged'))
  queue.enqueue(new Uint8Array(1), () => {})
  assert.deepEqual(discarded, [[OUTPUT_QUEUE_LIMIT + 1, 'overflow']])
  assert.equal(queue.pendingBytes, 0)
  f.advance(1000)
})

test('aged visible output is still parsed in order without pausing the display', () => {
  const f = fixture()
  const written = []
  const acknowledged = []
  let paused = 0
  const queue = new OutputQueue(f.scheduler, (data, done) => { written.push(data.length); done() }, () => true, () => paused++)
  queue.enqueue(new Uint8Array(16), () => acknowledged.push('old'))
  queue.enqueue(new Uint8Array(32), () => acknowledged.push('new'))
  f.cost(501)
  // Move the ready timer forward to model a suspended WebView event loop.
  for (const timer of f.timers.values()) timer.at = 501
  f.advance(0)
  assert.equal(paused, 0)
  assert.deepEqual(written, [16, 32])
  assert.deepEqual(acknowledged, ['old', 'new'])
  assert.equal(queue.pendingBytes, 0)
})

test('a slow parser keeps enough output per slice to make progress', () => {
  const f = fixture()
  const sizes = []
  const queue = new OutputQueue(f.scheduler, (data, done) => { sizes.push(data.length); f.cost(9); done() }, () => true, () => {})
  queue.enqueue(new Uint8Array(128 * 1024), () => {})
  f.advance(1000)
  assert.ok(sizes.length > 1)
  assert.ok(sizes.every((size) => size >= 4 * 1024), `Slices shrank below the floor: ${sizes.join(',')}`)
  queue.clear()
})

test('busy output still runs history scans within the maximum delay', () => {
  const f = fixture()
  let scans = 0
  const queue = new OutputQueue(f.scheduler, (_, done) => { f.cost(50); done() }, () => true, () => {})
  queue.scheduleMaintenance(() => { scans++; return false })
  queue.enqueue(new Uint8Array(OUTPUT_QUEUE_LIMIT), () => {})
  f.advance(2500)
  assert.equal(scans, 1, 'A streaming queue must not defer history comparison indefinitely')
  queue.clear()
  f.advance(10000)
  assert.equal(scans, 1)
})

test('a synchronous parser failure is reported separately from a timeout', () => {
  const f = fixture()
  const drops = []
  const queue = new OutputQueue(f.scheduler, () => { throw new Error('parser failed') }, () => true, (bytes, reason) => drops.push([bytes, reason]))
  queue.enqueue(new Uint8Array(16), () => {})
  f.advance(0)
  assert.deepEqual(drops, [[16, 'error']])
  assert.equal(queue.pendingBytes, 0)
})

test('hidden queues keep output while the window is in the background', () => {
  const f = fixture()
  const written = []
  let paused = 0
  const queue = new OutputQueue(f.scheduler, (data, done) => { written.push(data.length); done() }, () => false, () => paused++)
  queue.enqueue(new Uint8Array(16), () => {})
  f.cost(501)
  // Model the throttled event loop of a hidden window.
  for (const timer of f.timers.values()) timer.at = 501
  f.advance(0)
  assert.equal(paused, 0)
  assert.deepEqual(written, [16])
  assert.equal(queue.pendingBytes, 0)
})

test('cancellation removes scheduled history work and ignores old writes', () => {
  const f = fixture()
  let calls = 0
  const queue = new OutputQueue(f.scheduler, (_, done) => done(), () => true, () => {})
  queue.enqueue(new Uint8Array(16), () => calls++)
  queue.scheduleMaintenance(() => { calls++; return false })
  queue.clear()
  f.advance(1000)
  assert.equal(calls, 0)
  assert.equal(f.timers.size, 0)
})

test('real xterm preserves Chinese and ANSI sequences across scheduler chunks', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  t.after(() => terminal.dispose())
  const scheduler = new OutputScheduler()
  const queue = new OutputQueue(scheduler, (data, done) => terminal.write(data, done), () => true, () => assert.fail('unexpected overload'))
  const text = 'x'.repeat(8191) + '\x1b[31m中文🙂\x1b[0m\r\nfinished'
  await new Promise((resolve) => queue.enqueue(new TextEncoder().encode(text), resolve))
  const buffer = terminal.buffer.active
  const output = Array.from({ length: buffer.length }, (_, index) => buffer.getLine(index).translateToString(true)).join('')
  assert.ok(output.includes('中文🙂'))
  assert.ok(output.endsWith('finished'))
  assert.equal(queue.pendingBytes, 0)
})

test('history comparison yields and commits the same result as a complete scan', () => {
  const activity = new ContentActivity()
  const buffer = { type: 'normal', length: 5000, getLine: (index) => index < 5000 ? { isWrapped: false, translateToString: () => `row ${index}` } : undefined }
  const scan = activity.scan(buffer)
  assert.equal(scan.next().done, false)
  assert.equal(activity.unread, false, 'A partial scan must not change unread state')
  let result
  let slices = 1
  do { result = scan.next(); slices++ } while (!result.done)
  assert.ok(slices > 100)
  assert.equal(result.value, true)
  activity.markRead()
  assert.equal(activity.changed(buffer), false)
  assert.equal(activity.unread, false)
})
