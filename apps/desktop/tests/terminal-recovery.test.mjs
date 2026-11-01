/** Output acknowledgement and failure recovery regression tests. Author: jaswtao · Date: 2026-10-20 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { Channel } from '@tauri-apps/api/core'
import xterm from '@xterm/xterm'

const { OutputRecovery, AckQueue } = await loadService('terminal/recovery')
const flush = () => new Promise((resolve) => setTimeout(resolve, 25))
const { InputQueue } = await loadService('terminal/inputQueue')
const sudoInputModule = await loadService('terminal/sudoInput')
const frameModule = await loadService('terminal/frame')
const recoveryModule = await loadService('terminal/recovery')
const contentModule = await loadService('terminal/contentActivity')
const outputModule = await loadService('terminal/outputScheduler')
const directoryModule = await loadService('remote/directory')
const imeModule = await loadService('terminal/ime')

test('停止期间丢弃后续输入，旧写入失败不再误报到新会话', async () => {
  const errors = []
  const sent = []
  let reject
  const queue = new InputQueue((error) => errors.push(error))
  queue.setSender(() => new Promise((_, fail) => { reject = fail }))
  queue.push('already sent')
  queue.push('queued')
  queue.pause()
  queue.push('must not send')
  queue.resume(async (data) => { sent.push(data) })
  queue.push('new input')
  reject(new Error('old session exited'))
  await flush()
  assert.deepEqual(errors, [])
  assert.deepEqual(sent, ['new input'])
})

const clientSource = ts.transpileModule(await readFile(new URL('../src/services/terminal/terminalClient.ts', import.meta.url), 'utf8'), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText

function clientFixture(t, events = {}, clock = { setTimeout, clearTimeout, performance }, realTerminal = false) {
  const callbacks = new Map()
  let next = 0
  const previous = globalThis.window
  const window = globalThis.window = { setTimeout: clock.setTimeout, clearTimeout: clock.clearTimeout, __TAURI_INTERNALS__: {
    transformCallback(callback) { callbacks.set(++next, callback); return next }, unregisterCallback(id) { callbacks.delete(id) },
  } }
  const statuses = []
  const outputActivity = []
  const diagnostics = []
  const displays = []
  const api = { reportDiagnostic: async (...args) => diagnostics.push(args), ackTerminal: async () => {}, resizeTerminal: async () => {}, writeTerminal: async () => {}, pauseOutput: async () => {} }
  class Terminal {
    writes = []
    options = {}
    buffer = { active: { type: 'normal', length: 0, getLine: () => undefined } }
    oscHandlers = new Map()
    parser = { registerOscHandler: (code, handler) => this.oscHandlers.set(code, handler) }
    loadAddon() {} open() {} onResize() {} onWriteParsed() {} onData(callback) { this.data = callback }
    write(data, callback) { if (this.failWrite) throw new Error('parser failed'); this.writes.push(new TextDecoder().decode(data)); callback?.() }
    dispose() {} reset() {}
  }
  // Use the actual parser and buffers, with only layout and rendering events simulated.
  class ParsedTerminal extends xterm.Terminal {
    writes = []
    textarea = Object.assign(new EventTarget(), { style: {}, value: '' })
    composition = { style: {} }
    element = { querySelector: (selector) => selector === '.xterm-screen'
      ? { clientWidth: 800, clientHeight: 480 } : this.composition }
    loadAddon() {} open() {}
    onRender(callback) { return this.onWriteParsed(callback) }
    write(data, callback) {
      super.write(data, () => { this.writes.push(typeof data === 'string' ? data : new TextDecoder().decode(data)); callback?.() })
    }
  }
  const exports = {}
  const documentListeners = new Map()
  const document = {
    visibilityState: 'visible',
    addEventListener(name, handler) { documentListeners.set(name, [...(documentListeners.get(name) ?? []), handler]) },
    removeEventListener(name, handler) { documentListeners.set(name, (documentListeners.get(name) ?? []).filter((item) => item !== handler)) },
    emit(name) { for (const handler of documentListeners.get(name) ?? []) handler() },
  }
  vm.runInNewContext(clientSource, { exports, window, document, performance: clock.performance, queueMicrotask, ArrayBuffer,
    ResizeObserver: class { observe() {} disconnect() {} },
    require(name) {
      if (name === '@tauri-apps/api/core') return { Channel }
      if (name === '@xterm/addon-fit') return { FitAddon: class { fit() {} proposeDimensions() { return { cols: 80, rows: 24 } } } }
      if (name === '@xterm/xterm') return { Terminal: realTerminal ? ParsedTerminal : Terminal }
      if (name === '../terminalApi') return { terminalApi: api }
      if (name === './recovery') return recoveryModule
      if (name === './frame') return frameModule
      if (name === './inputQueue') return { InputQueue }
      if (name === './sudoInput') return sudoInputModule
      if (name === './ime') return realTerminal ? imeModule : { bindTerminalIme: () => ({ dispose() {}, reset() {}, recover() {}, refresh() {} }) }
      if (name === './selection') return { bindTerminalSelection: () => ({ dispose() {} }) }
      if (name === './shellIntegration') return { ShellIntegration: class { reset() {} input() {} } }
      if (name === '../remote/directory') return directoryModule
      if (name === './contentActivity') return contentModule
      if (name === './outputScheduler') return outputModule
      if (name === '../../i18n/translate') return { t: (key) => key }
      throw new Error(name)
    },
  })
  const container = { dataset: {}, offsetWidth: 0, offsetHeight: 0, contains: () => false }
  const client = new exports.TerminalClient(container, {}, { onStatus: (status) => statuses.push(status), onOutput: (unread) => outputActivity.push(unread), onDisplay: (paused, bytes) => displays.push({ paused, bytes }), ...events })
  const channel = client.createChannel()
  client.attach('session')
  let index = 0
  function deliver(kind, payload, fallback = false) {
    const data = typeof payload === 'string' ? new TextEncoder().encode(payload) : payload
    const frame = new ArrayBuffer(17 + data.length)
    new DataView(frame).setUint8(0, kind)
    new DataView(frame).setBigUint64(1, BigInt(index), true)
    new Uint8Array(frame, 17).set(data)
    callbacks.get(channel.id)({ index: index++, message: fallback ? Array.from(new Uint8Array(frame)) : frame })
  }
  t.after(() => { client.dispose(); globalThis.window = previous })
  return { client, deliver, statuses, outputActivity, diagnostics, displays, api, callbacks, channel, container, document }
}

for (const [name, hidden, glyph] of [
  ['inverse cursor', true, '\x1b[7m \x1b[0m'],
  ['RGB cursor', true, '\x1b[38;2;30;30;30;48;2;220;220;220m \x1b[0m'],
  ['hardware cursor', false, ' '],
]) test(`display recovery retains ${name} when redraw omits DECTCEM`, async (t) => {
  const f = clientFixture(t, {}, undefined, true)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 480
  f.client.term.resize(80, 24)
  f.client.updateStatus({ state: 'running' })
  const normal = '\x1b[38;2;220;220;220;48;2;30;30;30m'
  const redraw = `\x1b[2J\x1b[3;4H${normal}x${glyph}${normal}x\x1b[0m\x1b[24;80H`
  f.deliver(1, (hidden ? '\x1b[?25l' : '\x1b[?25h') + redraw)
  await waitForOutput(f, redraw)
  const { textarea } = f.client.term
  textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(textarea.style.left, hidden ? '40px' : '790px')
  assert.equal(textarea.style.top, hidden ? '40px' : '460px')
  f.api.listTerminalSessions = async () => [{ terminalId: 'session', status: { state: 'running' } }]
  const staleChannel = f.channel
  for (const background of [false, true]) {
    f.client.setVisible(!background)
    f.api.subscribeTerminal = async (_id, channel) => {
      // Late output from the old subscription must not change cursor modes.
      staleChannel.onmessage(outputFrame(1, hidden ? '\x1b[?25h' : '\x1b[?25l'))
      channel.onmessage(outputFrame(1, redraw))
    }
    f.client.pauseDisplay(1)
    await f.client.resumeDisplay()
    f.client.setVisible(true)
    textarea.dispatchEvent(new Event('compositionupdate'))
    assert.equal(textarea.style.left, hidden ? '40px' : '790px')
    assert.equal(textarea.style.top, hidden ? '40px' : '460px')
    const cell = f.client.term.buffer.active.getLine(2).getCell(4)
    if (name === 'inverse cursor') assert.ok(cell.isInverse())
    if (name === 'RGB cursor') assert.equal(cell.getBgColor(), 0xdcdcdc)
  }
  await new Promise((resolve) => f.client.term.write('\x1b[?25h\x1b[5;7H', resolve))
  textarea.dispatchEvent(new Event('compositionend'))
  assert.equal(textarea.style.left, '60px')
  assert.equal(textarea.style.top, '80px')
})

test('remote clients deliver directory reports without enabling local command tracking', (t) => {
  const events = []
  const prompts = []
  const f = clientFixture(t, { shellIntegration: false, onShell: (event) => events.push(event), onRemotePrompt: (event) => prompts.push(event) })
  assert.equal(f.client.term.oscHandlers.has(133), false)
  f.client.term.oscHandlers.get(9)('9;/home/中文 space')
  f.client.term.oscHandlers.get(7)('file://remote.example/var/log')
  f.client.term.oscHandlers.get(9)('9;relative')
  assert.deepEqual(prompts, ['cwd'])
  assert.deepEqual(events.map((event) => ({ ...event })), [{ type: 'cwd', path: '/home/中文 space' }, { type: 'cwd', path: '/var/log' }])
})

test('remote sudo frames offer physical Enter while paste and manual input use ordinary writes', async (t) => {
  const prompts = [], submissions = [], written = []
  const f = clientFixture(t, { shellIntegration: false, onSudoPrompt: (available) => prompts.push(available) })
  f.api.submitSudoSavedPassword = async (...args) => { submissions.push(args) }
  f.api.writeTerminal = async (_, data) => { written.push(data) }
  f.deliver(2, '{"state":"running"}')
  f.deliver(5, '{"promptId":"first"}')
  assert.equal(prompts.at(-1), true)
  const key = () => ({ type: 'keydown', key: 'Enter', keyCode: 13, preventDefault() {}, stopPropagation() {} })
  assert.equal(f.client.handleSudoKey(key()), false)
  assert.equal(f.client.handleSudoKey(key()), false)
  await flush()
  assert.deepEqual(submissions, [['session', 'first']])
  assert.deepEqual(written, [])
  f.deliver(5, '{"promptId":"second"}')
  f.client.term.data('\r')
  assert.equal(prompts.at(-1), false)
  assert.equal(f.client.handleSudoKey(key()), true)
  await flush()
  assert.deepEqual(written, ['\r'])
  assert.equal(submissions.length, 1)
  f.deliver(5, '{"promptId":"third"}')
  f.client.term.data('manual\x7f')
  assert.equal(prompts.at(-1), false)
  f.deliver(5, '{"promptId":null}')
  f.deliver(2, '{"state":"exited","exitCode":0}')
  assert.equal(prompts.at(-1), false)
})

test('failed sudo IPC reports a localized error without forwarding an empty Enter', async (t) => {
  const errors = [], written = []
  const f = clientFixture(t, { shellIntegration: false, onError: (error) => errors.push(error) })
  f.api.submitSudoSavedPassword = async () => { throw new Error('missing credential') }
  f.api.writeTerminal = async (_, data) => { written.push(data) }
  f.deliver(2, '{"state":"running"}')
  f.deliver(5, '{"promptId":"first"}')
  f.client.handleSudoKey({ type: 'keydown', key: 'Enter', keyCode: 13, preventDefault() {}, stopPropagation() {} })
  await flush()
  assert.deepEqual(errors, ['remote.sudoPasswordFailed'])
  assert.deepEqual(written, [])
})

test('sudo metadata never enables saved-password handling in a local terminal', (t) => {
  const prompts = []
  const f = clientFixture(t, { onSudoPrompt: (available) => prompts.push(available) })
  f.deliver(2, '{"state":"running"}')
  f.deliver(5, '{"promptId":"first"}')
  assert.equal(prompts.includes(true), false)
})

test('终端回传恢复已读内容的状态，真实输出保持未读直到查看', (t) => {
  const f = clientFixture(t)
  let text = 'Codex waiting for input'
  f.client.term.buffer.active = { type: 'normal', length: 1, getLine: () => ({ translateToString: () => text }) }
  f.client.flushContentActivity()
  f.client.markContentRead()
  text = ''
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), true)
  text = 'Codex waiting for input'
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), false)
  text += '\nnew output'
  f.client.flushContentActivity()
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), true)
  f.client.markContentRead()
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), false)
})

test('reconnecting after hiding preserves unread output received while subscribing', async (t) => {
  const f = clientFixture(t)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 600
  let text = 'seen output'
  f.client.term.buffer.active = { type: 'normal', length: 1, getLine: () => ({ translateToString: () => text }) }
  f.client.flushContentActivity()
  f.client.markContentRead()
  f.deliver(2, '{"state":"running"}')
  f.api.listTerminalSessions = async () => [{ terminalId: 'session', status: { state: 'running' } }]
  let releaseSubscribe
  f.api.subscribeTerminal = async (_id, channel) => {
    await new Promise((resolve) => { releaseSubscribe = resolve })
    channel.onmessage(outputFrame(1, 'redraw'))
  }

  const pending = f.client.reconnect()
  await flush()
  assert.equal(typeof releaseSubscribe, 'function')
  f.container.offsetWidth = 0
  text += '\nbackground result'
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), true)
  releaseSubscribe()
  await pending
  assert.equal(f.client.connection, 'connected')
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), true)

  text = 'seen output'
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), false)
  text += '\nnew result'
  f.client.flushContentActivity()
  f.client.markContentRead()
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), false)
})

test('attaching a new session resets the previous unread baseline', (t) => {
  const f = clientFixture(t)
  f.client.term.buffer.active = { type: 'normal', length: 1, getLine: () => ({ translateToString: () => 'old output' }) }
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), true)
  f.client.attach('new-session')
  f.client.flushContentActivity()
  assert.equal(f.outputActivity.at(-1), false)
})

test('真实 Tauri 有序 Channel 中，坏 JSON 后仍能接收退出状态', (t) => {
  const f = clientFixture(t)
  assert.doesNotThrow(() => f.deliver(2, '{broken'))
  f.deliver(2, '{"state":"exited","exitCode":0}')
  assert.equal(f.client.status.state, 'exited')
  assert.equal(f.client.term.options.disableStdin, true)
  assert.equal(f.statuses.at(-1).exitCode, 0)
})

test('写入异常不会锁死 Channel，状态查询可绕过输出直接标记退出', (t) => {
  const f = clientFixture(t)
  f.deliver(2, '{"state":"running"}')
  f.client.term.failWrite = true
  assert.doesNotThrow(() => f.deliver(1, 'output'))
  f.client.observeSession({ terminalId: 'session', status: { state: 'exited', exitCode: 1 }, output: {} })
  assert.equal(f.client.status.state, 'exited')
  assert.equal(f.client.term.options.disableStdin, true)
  f.client.updateStatus({ state: 'running' })
  assert.equal(f.client.status.state, 'exited', '迟到的运行状态不能复活旧会话')
})

test('独立状态查询先报告退出时，仍确认迟到的尾部输出', async (t) => {
  const f = clientFixture(t)
  const confirmed = []
  f.api.ackTerminal = async (id, seq) => confirmed.push([id, seq])
  f.client.updateStatus({ state: 'exited', exitCode: 0 })
  f.deliver(1, 'last output')
  await flush()
  await f.client.flushAck()
  assert.equal(confirmed.length, 1)
  assert.equal(f.client.term.options.disableStdin, true)
})

test('停止请求失败且查询暂不可用时，后续真实状态决定是否恢复输入', (t) => {
  const f = clientFixture(t)
  f.client.updateStatus({ state: 'running' })
  f.client.pauseInput()
  f.client.waitForStatus()
  assert.equal(f.client.term.options.disableStdin, true)
  f.client.observeSession({ terminalId: 'session', status: { state: 'running' }, output: { subscribed: true, inflightBytes: 0, ackIdleMs: null } })
  assert.equal(f.client.term.options.disableStdin, false)
})

function outputFrame(kind, text) {
  const payload = new TextEncoder().encode(text)
  const buffer = new ArrayBuffer(17 + payload.length)
  new DataView(buffer).setUint8(0, kind)
  new Uint8Array(buffer, 17).set(payload)
  return buffer
}

async function waitForOutput(fixture, suffix) {
  const deadline = performance.now() + 2000
  while (!fixture.client.term.writes.join('').endsWith(suffix) && performance.now() < deadline) await flush()
  assert.ok(fixture.client.term.writes.join('').endsWith(suffix))
}

function displayClock() {
  let now = 0
  let next = 0
  const timers = new Map()
  return {
    performance: { now: () => now },
    setTimeout(action, delay) { const id = ++next; timers.set(id, { action, at: now + delay }); return id },
    clearTimeout(id) { timers.delete(id) },
    advance(ms) {
      const end = now + ms
      while (true) {
        const entry = [...timers].sort((a, b) => a[1].at - b[1].at)[0]
        if (!entry || entry[1].at > end) break
        timers.delete(entry[0])
        now = entry[1].at
        entry[1].action()
      }
      now = end
    },
  }
}

function automaticDisplayFixture(t) {
  const clock = displayClock()
  const f = clientFixture(t, {}, clock)
  let subscriptions = 0
  f.api.listTerminalSessions = async () => [{ terminalId: f.client.id, status: { state: 'running' } }]
  f.api.subscribeTerminal = async (_id, channel) => {
    subscriptions++
    channel.onmessage(outputFrame(2, '{"state":"running"}'))
    channel.onmessage(outputFrame(1, 'fresh redraw'))
  }
  f.client.updateStatus({ state: 'running' })
  return { ...f, clock, subscriptions: () => subscriptions }
}

test('display resumes automatically in the background and backs off until stable', async (t) => {
  const f = automaticDisplayFixture(t)
  f.client.setVisible(false)
  for (const delay of [1000, 2000, 4000, 8000, 10000, 10000]) {
    f.client.pauseDisplay(42)
    const before = f.subscriptions()
    f.clock.advance(delay - 1)
    await flush()
    assert.equal(f.subscriptions(), before)
    f.clock.advance(1)
    await flush()
    assert.equal(f.subscriptions(), before + 1)
    assert.equal(f.displays.at(-1).paused, false)
    assert.equal(f.client.id, 'session')
  }
  f.clock.advance(30000)
  f.client.pauseDisplay(1)
  f.clock.advance(1000)
  await flush()
  assert.equal(f.subscriptions(), 7, 'Stable display resets the retry interval')
})

test('hidden windows suspend automatic display recovery until the window returns', async (t) => {
  const f = automaticDisplayFixture(t)
  f.document.visibilityState = 'hidden'
  f.document.emit('visibilitychange')
  f.client.pauseDisplay(1)
  f.clock.advance(120000)
  await flush()
  assert.equal(f.subscriptions(), 0, 'A hidden window must not reconnect without rendering output')
  f.document.visibilityState = 'visible'
  f.document.emit('visibilitychange')
  await flush()
  assert.equal(f.subscriptions(), 1)
  assert.equal(f.displays.at(-1).paused, false)
})

test('showing a paused terminal resumes before the retry timer fires', async (t) => {
  const f = automaticDisplayFixture(t)
  f.client.setVisible(false)
  f.client.pauseDisplay(1)
  f.clock.advance(500)
  await flush()
  assert.equal(f.subscriptions(), 0)
  f.client.setVisible(true)
  await flush()
  assert.equal(f.subscriptions(), 1)
  assert.equal(f.displays.at(-1).paused, false)
})

test('resuming a paused display asks the program to repaint once', async (t) => {
  const f = automaticDisplayFixture(t)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 600
  const nudges = []
  f.api.resizeTerminal = async (id, size, nudge) => { nudges.push([id, size.cols, size.rows, nudge]) }
  f.client.pauseDisplay(1)
  f.clock.advance(1000)
  await flush()
  assert.deepEqual(nudges.filter((item) => item[3] === true), [['session', 80, 24, true]])
  assert.equal(f.displays.at(-1).paused, false)
})

test('hiding the window cancels a pending display retry', async (t) => {
  const f = automaticDisplayFixture(t)
  f.client.pauseDisplay(1)
  f.clock.advance(999)
  await flush()
  assert.equal(f.subscriptions(), 0)
  f.document.visibilityState = 'hidden'
  f.document.emit('visibilitychange')
  f.clock.advance(120000)
  await flush()
  assert.equal(f.subscriptions(), 0, 'A canceled retry must not reconnect in the background')
  f.document.visibilityState = 'visible'
  f.document.emit('visibilitychange')
  await flush()
  assert.equal(f.subscriptions(), 1)
  assert.equal(f.displays.at(-1).paused, false)
})

test('a reconnect finishing after the window returns re-arms display recovery', async (t) => {
  const f = automaticDisplayFixture(t)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 600
  const subscribe = f.api.subscribeTerminal
  const resize = f.api.resizeTerminal
  let releaseResize
  f.api.subscribeTerminal = async (_id, channel) => {
    channel.onmessage(outputFrame(2, '{"state":"running"}'))
    channel.onmessage(outputFrame(1, 'x'.repeat(outputModule.OUTPUT_QUEUE_LIMIT + 1)))
  }
  f.api.resizeTerminal = () => new Promise((resolve) => { releaseResize = resolve })
  const pending = f.client.reconnect()
  for (let attempt = 0; attempt < 20 && !releaseResize; attempt++) await flush()
  assert.equal(typeof releaseResize, 'function', 'Reconnect must reach the resize nudge')
  assert.equal(f.displays.at(-1).paused, true)
  f.document.visibilityState = 'hidden'
  f.document.emit('visibilitychange')
  f.document.visibilityState = 'visible'
  f.document.emit('visibilitychange')
  f.api.subscribeTerminal = subscribe
  f.api.resizeTerminal = resize
  releaseResize()
  await pending
  f.clock.advance(1000)
  for (let attempt = 0; attempt < 20 && f.displays.at(-1).paused !== false; attempt++) await flush()
  assert.equal(f.subscriptions(), 1, 'The retry chain must survive a visibility change during reconnect')
  assert.equal(f.displays.at(-1).paused, false)
})

test('automatic display recovery waits for pause completion and shares the manual lock', async (t) => {
  const f = automaticDisplayFixture(t)
  let release
  f.api.pauseOutput = () => new Promise((resolve) => { release = resolve })
  f.client.pauseDisplay(1)
  f.clock.advance(1000)
  await f.client.resumeDisplay()
  await flush()
  assert.equal(f.subscriptions(), 0)
  release()
  await flush()
  assert.equal(f.subscriptions(), 1)
  f.clock.advance(10000)
  await flush()
  assert.equal(f.subscriptions(), 1)
})

test('overload during a replacement subscription schedules another automatic recovery', async (t) => {
  const f = automaticDisplayFixture(t)
  const subscribe = f.api.subscribeTerminal
  f.api.subscribeTerminal = async (_id, channel) => {
    channel.onmessage(outputFrame(2, '{"state":"running"}'))
    channel.onmessage(outputFrame(1, 'x'.repeat(outputModule.OUTPUT_QUEUE_LIMIT + 1)))
  }
  f.client.pauseDisplay(1)
  f.clock.advance(1000)
  await flush()
  assert.equal(f.displays.at(-1).paused, true)
  assert.equal(f.client.reconnecting, false)
  f.api.subscribeTerminal = subscribe
  f.clock.advance(2000)
  await flush()
  assert.equal(f.subscriptions(), 1)
  assert.equal(f.displays.at(-1).paused, false)
})

test('failed display recovery keeps retrying and manual recovery cancels its timer', async (t) => {
  const f = automaticDisplayFixture(t)
  const subscribe = f.api.subscribeTerminal
  let failures = 0
  f.api.subscribeTerminal = async () => { failures++; throw new Error('temporarily unavailable') }
  f.client.pauseDisplay(1)
  for (const delay of [1000, 2000, 4000, 8000, 10000]) {
    f.clock.advance(delay)
    await flush()
    assert.equal(f.displays.at(-1).paused, true)
  }
  assert.equal(failures, 5)
  f.api.subscribeTerminal = subscribe
  await f.client.resumeDisplay()
  f.clock.advance(10000)
  await flush()
  assert.equal(f.subscriptions(), 1)
  assert.equal(f.displays.at(-1).paused, false)
})

test('display retry is cancelled on stop, exit, disposal and session replacement', async (t) => {
  for (const action of ['stop', 'exit', 'dispose', 'replace']) {
    const f = automaticDisplayFixture(t)
    f.client.pauseDisplay(1)
    if (action === 'stop') f.client.pauseInput()
    if (action === 'exit') f.client.updateStatus({ state: 'exited', exitCode: 0 })
    if (action === 'dispose') f.client.dispose()
    if (action === 'replace') f.client.attach('replacement')
    f.clock.advance(20000)
    await flush()
    assert.equal(f.subscriptions(), 0, action)
    if (action === 'stop') {
      f.client.resumeInput()
      f.clock.advance(1000)
      await flush()
      assert.equal(f.subscriptions(), 1, 'An unsuccessful stop may resume automatic recovery')
    }
  }
})

test('a late pause cannot resume a replaced session', async (t) => {
  const f = automaticDisplayFixture(t)
  let release
  f.api.pauseOutput = () => new Promise((resolve) => { release = resolve })
  f.client.pauseDisplay(1)
  f.clock.advance(1000)
  f.client.attach('replacement')
  f.client.updateStatus({ state: 'running' })
  release()
  await flush()
  assert.equal(f.subscriptions(), 0)
  assert.equal(f.client.id, 'replacement')
  assert.equal(f.displays.at(-1).paused, false)
})

test('overload pauses only display, keeps input alive, and resumes through an explicit subscription', async (t) => {
  const f = clientFixture(t)
  const paused = []
  const inputs = []
  let releasePause
  f.api.pauseOutput = (id) => { paused.push(id); return new Promise((resolve) => { releasePause = resolve }) }
  f.api.writeTerminal = async (id, data) => inputs.push([id, data])
  f.deliver(2, '{"state":"running"}')
  f.client.term.failWrite = true
  f.deliver(1, 'unrendered')
  await flush()
  assert.deepEqual(paused, ['session'])
  assert.equal(f.displays.at(-1).paused, true)
  assert.equal(f.client.term.options.disableStdin, false)
  f.client.term.data('\x03')
  await flush()
  assert.deepEqual(inputs, [['session', '\x03']])
  f.deliver(1, 'dropped')
  f.client.observeSession({ terminalId: 'session', status: { state: 'running' }, output: {
    paused: true, droppedBytes: 100, subscribed: true, inflightBytes: 0, ackIdleMs: null,
  } })
  assert.equal(f.displays.at(-1).bytes, 117)
  assert.equal(f.diagnostics.some((args) => args[1] === 'reconnect_started'), false)

  f.client.term.failWrite = false
  let subscriptions = 0
  f.api.listTerminalSessions = async () => [{ terminalId: 'session', status: { state: 'running' } }]
  f.api.subscribeTerminal = async (_id, channel) => {
    subscriptions++
    channel.onmessage(outputFrame(2, '{"state":"running"}'))
    channel.onmessage(outputFrame(1, 'fresh redraw'))
  }
  const resumed = f.client.resumeDisplay()
  await f.client.resumeDisplay()
  await flush()
  assert.equal(subscriptions, 0, 'Resume must wait for the pause RPC to finish')
  releasePause()
  await resumed
  assert.equal(subscriptions, 1)
  assert.equal(f.displays.at(-1).paused, false)
  assert.equal(f.client.id, 'session', 'Resuming display must not restart the task')
  assert.ok(f.client.term.writes.join('').endsWith('fresh redraw'))
})

test('a timed-out parser cannot starve another client or acknowledge a late write', async (t) => {
  const previousWindow = globalThis.window
  const broken = clientFixture(t)
  const normal = clientFixture(t)
  t.after(() => { globalThis.window = previousWindow })
  const acknowledgements = []
  let late
  broken.client.term.write = (_, complete) => { late = complete }
  broken.api.ackTerminal = async (_, seq) => acknowledgements.push(seq)
  broken.deliver(2, '{"state":"running"}')
  normal.deliver(2, '{"state":"running"}')
  broken.deliver(1, 'blocked parser')
  normal.deliver(1, 'other terminal remains usable')
  await waitForOutput(normal, 'other terminal remains usable')
  assert.equal(broken.displays.at(-1).paused, true)
  assert.ok(broken.diagnostics.some((args) => args[1] === 'output_timeout' && typeof args[4] === 'number'))
  assert.equal(normal.displays.at(-1).paused, false)
  late()
  await flush()
  assert.deepEqual(acknowledgements, [])
})

test('a local queue overflow reports output_overflow with the discarded bytes', async (t) => {
  const f = clientFixture(t)
  f.client.term.write = () => {}
  f.deliver(2, '{"state":"running"}')
  for (let index = 0; index < 17; index++) f.deliver(1, new Uint8Array(16 * 1024))
  const diagnostic = f.diagnostics.find((args) => args[1] === 'output_overflow')
  assert.ok(diagnostic)
  assert.equal(diagnostic[4], 17 * 16 * 1024)
  assert.equal(f.displays.at(-1).paused, true)
})

test('output failure still forwards Escape and Ctrl+C to the running process', async (t) => {
  const f = clientFixture(t)
  const inputs = []
  f.api.writeTerminal = async (id, data) => inputs.push([id, data])
  f.deliver(2, '{"state":"running"}')
  f.deliver(2, '{broken')
  assert.equal(f.client.connection, 'disconnected')
  assert.equal(f.client.term.options.disableStdin, false)
  f.client.term.data('\x1b')
  await flush()
  f.client.term.data('\x03')
  await flush()
  assert.deepEqual(inputs, [['session', '\x1b'], ['session', '\x03']])
  f.client.pauseInput()
  f.client.term.data('must not reach a stopped session')
  await flush()
  assert.equal(inputs.length, 2)
})

test('output resubscription preserves queued input and keeps writes serialized', async (t) => {
  const f = clientFixture(t)
  const inputs = []
  let release
  f.api.writeTerminal = (id, data) => {
    inputs.push([id, data])
    return inputs.length === 1 ? new Promise((resolve) => { release = resolve }) : Promise.resolve()
  }
  f.deliver(2, '{"state":"running"}')
  f.client.term.data('first')
  f.client.term.data('\x03')
  f.client.createChannel()
  f.client.attach('session')
  f.client.term.data('\x1b')
  assert.deepEqual(inputs, [['session', 'first']])
  release()
  await flush()
  assert.deepEqual(inputs, [['session', 'first'], ['session', '\x03\x1b']])
})

test('output reconnect forwards interrupts while waiting for a subscription and after failure', async (t) => {
  const f = clientFixture(t)
  const inputs = []
  f.api.writeTerminal = async (id, data) => inputs.push([id, data])
  f.deliver(2, '{"state":"running"}')
  f.api.listTerminalSessions = async () => [{ terminalId: 'session', status: { state: 'running' } }]
  let rejectSubscribe
  f.api.subscribeTerminal = () => new Promise((_, reject) => { rejectSubscribe = reject })
  const reconnect = f.client.reconnect()
  await flush()
  assert.equal(f.client.connection, 'reconnecting')
  f.client.term.data('\x1b')
  await flush()
  rejectSubscribe(new Error('subscription failed'))
  await reconnect
  assert.equal(f.client.connection, 'disconnected')
  f.client.term.data('\x03')
  await flush()
  assert.deepEqual(inputs, [['session', '\x1b'], ['session', '\x03']])
  f.client.updateStatus({ state: 'exited', exitCode: 0 })
  f.client.term.data('must not reach an exited process')
  await flush()
  assert.equal(inputs.length, 2)
})

test('IPC fallback byte arrays keep output acknowledged and input usable after reconnect', async (t) => {
  const f = clientFixture(t)
  const confirmed = []
  const inputs = []
  f.api.writeTerminal = async (id, data) => inputs.push([id, data])
  f.api.ackTerminal = async (_id, seq) => confirmed.push(seq)
  f.deliver(2, '{"state":"running"}')
  f.deliver(1, 'Codex redraw '.repeat(1024), true)
  await waitForOutput(f, 'Codex redraw '.repeat(1024))
  await f.client.flushAck()
  assert.equal(f.client.connection, 'connected')
  assert.equal(f.client.term.options.disableStdin, false)
  assert.deepEqual(confirmed, [1])
  assert.equal(f.diagnostics.some((args) => args[1] === 'frame_error'), false)
  assert.ok(f.client.term.writes.join('').endsWith('Codex redraw '.repeat(1024)))
  f.client.term.data('draft')
  await flush()
  assert.deepEqual(inputs, [['session', 'draft']])

  f.api.listTerminalSessions = async () => [{ terminalId: 'session', status: { state: 'running' } }]
  f.api.subscribeTerminal = async (_id, channel) => {
    channel.onmessage(outputFrame(1, '\x1b[?25l'))
    channel.onmessage(Array.from(new Uint8Array(outputFrame(1, 'Codex redraw '.repeat(1024)))))
  }
  await f.client.reconnect()
  await waitForOutput(f, 'Codex redraw '.repeat(1024))
  await f.client.flushAck()
  f.deliver(1, 'ignored old subscription', true)
  assert.equal(f.client.connection, 'connected')
  assert.equal(f.client.term.options.disableStdin, false)
  assert.equal(f.diagnostics.some((args) => args[1] === 'frame_error'), false)
  assert.ok(f.client.term.writes.join('').endsWith('Codex redraw '.repeat(1024)))
  f.client.term.data('after reconnect')
  await flush()
  assert.deepEqual(inputs.at(-1), ['session', 'after reconnect'])
})

test('fallback frames reject invalid bytes rather than silently coercing them', () => {
  for (const byte of [-1, 256, 1.5, '1', null, NaN]) {
    const frame = Array.from(new Uint8Array(outputFrame(1, 'text')))
    frame[17] = byte
    assert.throws(() => frameModule.decodeFrame(frame))
  }
})

test('background output and IPC fallback do not disable input when returning from a browser', async (t) => {
  const f = clientFixture(t)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 600
  const inputs = []
  const confirmed = []
  f.api.writeTerminal = async (id, data) => inputs.push([id, data])
  f.api.ackTerminal = async (_id, seq) => confirmed.push(seq)
  f.deliver(2, '{"state":"running"}')
  f.document.visibilityState = 'hidden'
  f.client.observeSession({ terminalId: 'session', status: { state: 'running' },
    output: { subscribed: true, inflightBytes: 12000, ackIdleMs: 60000 } })
  f.deliver(1, 'Browser operation completed\r\n'.repeat(512), true)
  await waitForOutput(f, 'Browser operation completed\r\n'.repeat(512))
  await f.client.flushAck()

  f.document.visibilityState = 'visible'
  f.client.observeSession({ terminalId: 'session', status: { state: 'running' },
    output: { subscribed: true, inflightBytes: 0, ackIdleMs: null } })
  f.deliver(1, '\x1b[?25l')
  f.deliver(1, 'Codex redraw after return\r\n'.repeat(512), true)
  await waitForOutput(f, 'Codex redraw after return\r\n'.repeat(512))
  await f.client.flushAck()
  f.client.term.data('next question')
  await flush()
  assert.equal(f.client.connection, 'connected')
  assert.equal(f.client.term.options.disableStdin, false)
  assert.deepEqual(confirmed, [1, 3])
  assert.deepEqual(inputs, [['session', 'next question']])
  assert.ok(f.client.term.writes.join('').endsWith('Codex redraw after return\r\n'.repeat(512)))
  assert.deepEqual(f.diagnostics, [])
})

test('帧异常自动恢复一次，后续异常保留手动重连入口且不重启会话', async (t) => {
  const f = clientFixture(t)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 600
  const health = { subscribed: true, inflightBytes: 0, ackIdleMs: null }
  f.api.listTerminalSessions = async () => [{ terminalId: 'session', status: { state: 'running' }, output: health }]
  const nudges = []
  f.api.resizeTerminal = async (id, size, nudge) => { nudges.push([id, size.cols, size.rows, nudge]) }
  let subscriptions = 0
  let newest
  function frame(kind, text) {
    const payload = new TextEncoder().encode(text)
    const buffer = new ArrayBuffer(17 + payload.length)
    new DataView(buffer).setUint8(0, kind)
    new Uint8Array(buffer, 17).set(payload)
    return buffer
  }
  f.api.subscribeTerminal = async (id, channel) => {
    assert.equal(id, 'session')
    subscriptions++
    newest = channel
    channel.onmessage(frame(2, '{"state":"running"}'))
    channel.onmessage(frame(1, '恢复后的输出'))
  }
  f.deliver(2, '{"state":"running"}')
  f.deliver(2, '{bad json')
  await flush()
  assert.equal(subscriptions, 1)
  assert.equal(f.client.term.options.disableStdin, false)
  assert.equal(nudges.some((item) => item[3] === true), false, 'A fault reconnect must not nudge the program to repaint')
  assert.ok(nudges.some((item) => item[3] === false))
  assert.ok(f.diagnostics.some((args) => args[1] === 'reconnect_succeeded'))
  newest.onmessage(frame(2, '{bad again'))
  await flush()
  assert.equal(subscriptions, 1)
  assert.equal(f.client.connection, 'disconnected')
  assert.equal(f.client.term.options.disableStdin, false)
  await f.client.reconnect()
  assert.equal(subscriptions, 2)
  assert.equal(f.client.id, 'session')
  assert.equal(f.client.term.options.disableStdin, false)
  assert.equal(nudges.some((item) => item[3] === true), false)
})

test('停止取消进行中的重连后，连接回到可手动重连且不报恢复失败', async (t) => {
  const f = clientFixture(t)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 600
  f.deliver(2, '{"state":"running"}')
  const running = { terminalId: 'session', status: { state: 'running' }, output: { subscribed: true, inflightBytes: 0, ackIdleMs: null } }
  let releaseList
  f.api.listTerminalSessions = () => new Promise((resolve) => { releaseList = resolve })
  const pending = f.client.reconnect()
  await flush()
  assert.equal(f.client.connection, 'reconnecting')
  f.client.pauseInput()
  releaseList([running])
  await pending
  assert.equal(f.client.connection, 'disconnected')
  assert.equal(f.diagnostics.some((args) => args[1] === 'reconnect_failed'), false)
  f.client.resumeInput()
  assert.equal(f.client.term.options.disableStdin, false)

  f.api.listTerminalSessions = async () => [running]
  f.api.subscribeTerminal = async (_id, channel) => {
    channel.onmessage(outputFrame(2, '{"state":"running"}'))
    channel.onmessage(outputFrame(1, '再次恢复'))
  }
  await f.client.reconnect()
  assert.equal(f.client.connection, 'connected')
  assert.equal(f.client.term.options.disableStdin, false)
})

test('重连已发出订阅但被停止取消时，不保持正在重连', async (t) => {
  const f = clientFixture(t)
  f.container.offsetWidth = 800
  f.container.offsetHeight = 600
  f.deliver(2, '{"state":"running"}')
  let releaseSubscribe
  f.api.listTerminalSessions = async () => [{ terminalId: 'session', status: { state: 'running' }, output: { subscribed: true, inflightBytes: 0, ackIdleMs: null } }]
  f.api.subscribeTerminal = () => new Promise((resolve) => { releaseSubscribe = resolve })
  const pending = f.client.reconnect()
  for (let i = 0; i < 5 && !releaseSubscribe; i++) await flush()
  assert.equal(typeof releaseSubscribe, 'function')
  f.client.pauseInput()
  releaseSubscribe()
  await pending
  assert.equal(f.client.connection, 'disconnected')
  assert.equal(f.client.term.options.disableStdin, true)
  f.client.resumeInput()
  assert.equal(f.client.term.options.disableStdin, false)
})

test('确认失败后保留进度，串行重试时合并新到达的确认', async () => {
  let reject
  const calls = []
  const ack = new AckQueue((seq) => {
    calls.push(seq)
    return calls.length === 1 ? new Promise((_, fail) => { reject = fail }) : Promise.resolve()
  })
  ack.processed(1, 100)
  const first = ack.flush()
  ack.processed(2, 50)
  await ack.flush()
  assert.deepEqual(calls, [1])
  reject(new Error('offline'))
  assert.equal(await first, false)
  assert.equal(ack.bytes, 150)
  assert.equal(await ack.flush(), true)
  assert.equal(ack.bytes, 0)
  assert.deepEqual(calls, [1, 2])
})

test('旧确认完成不影响重置后的订阅', async () => {
  let release
  const ack = new AckQueue(() => new Promise((resolve) => { release = resolve }))
  ack.processed(1, 10)
  const pending = ack.flush()
  ack.reset()
  ack.processed(20, 30)
  release()
  await pending
  assert.equal(ack.bytes, 30)
})

test('空闲、隐藏与短暂确认延迟不会重连，持续停滞仅自动重连一次', () => {
  const recovery = new OutputRecovery()
  const healthy = { subscribed: true, inflightBytes: 0, ackIdleMs: null }
  const stalled = { subscribed: true, inflightBytes: 1024, ackIdleMs: 20000 }
  assert.equal(recovery.observe(healthy, true, 0), false)
  assert.equal(recovery.observe(stalled, false, 10000), false)
  assert.equal(recovery.observe(stalled, true, 10000), false)
  assert.equal(recovery.observe(stalled, true, 18000), false)
  assert.equal(recovery.observe(stalled, true, 20000), true)
  assert.equal(recovery.observe(stalled, true, 50000), false)
  recovery.reset()
  assert.equal(recovery.fault(true), true)
  assert.equal(recovery.fault(true), false)
})

test('隐藏期间的异常在重新可见后恢复，不消耗多次自动恢复额度', async () => {
  const recovery = new OutputRecovery()
  assert.equal(recovery.fault(false), false)
  assert.equal(recovery.observe({ subscribed: true, inflightBytes: 0, ackIdleMs: null }, true, 0), true)
  await flush()
  assert.equal(recovery.fault(true), false)
})

const registrySource = await readFile(new URL('../src/services/terminal/registry.ts', import.meta.url), 'utf8')

function loadRegistry(terminalApi) {
  const source = ts.transpileModule(registrySource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText
  const exports = {}
  vm.runInNewContext(source, {
    exports,
    require(name) {
      if (name === '../appearance') return { terminalAppearance: () => ({}) }
      if (name === '../remote/directory') return directoryModule
      if (name === './recovery') return recoveryModule
      if (name === '../terminalApi') return { terminalApi }
      if (name === 'vue') return { markRaw: (value) => value, shallowReactive: (value) => value }
      if (name === '../workspace/shortcuts') return { matchShortcut: () => null }
      if (name === '../../i18n/translate') return { t: (key) => key }
      if (name === './terminalClient') return { TerminalClient: class {} }
      throw new Error(name)
    },
  })
  return exports.TerminalRegistry
}

test('会话查询失败会记到每个会话，且失败后仍可再次查询', async () => {
  const reports = []
  const TerminalRegistry = loadRegistry({ async listTerminalSessions() { throw new Error('down') } })
  const registry = new TerminalRegistry(() => 'dark', () => ({}))
  for (const id of ['a', 'b']) {
    registry.byConfig.set(id, { client: { id, reportPollFailed() { reports.push(id) } } })
  }
  await registry.syncSessions()
  assert.deepEqual(reports, ['a', 'b'])
  reports.length = 0
  await registry.syncSessions()
  assert.deepEqual(reports, ['a', 'b'])
})

test('挂起的会话查询不阻止下一次，过期结果不会覆盖新状态', async () => {
  const calls = []
  let releaseFirst
  let releaseSecond
  const seen = []
  const TerminalRegistry = loadRegistry({
    listTerminalSessions() {
      const index = calls.push(calls.length + 1)
      if (index === 1) return new Promise((resolve) => { releaseFirst = resolve })
      if (index === 2) return new Promise((resolve) => { releaseSecond = resolve })
      return Promise.resolve([])
    },
  })
  const registry = new TerminalRegistry(() => 'dark', () => ({}))
  const health = { subscribed: true, inflightBytes: 0, ackIdleMs: null }
  registry.byConfig.set('cfg', { client: {
    id: 'session',
    observeSession(session) { seen.push(session.status.state) },
    updateStatus(status) { seen.push(status.state) },
    reportPollFailed() { seen.push('poll') },
  } })
  const first = registry.syncSessions()
  const second = registry.syncSessions()
  const third = registry.syncSessions()
  await flush()
  assert.equal(calls.length, 2)
  releaseSecond([{ terminalId: 'session', status: { state: 'running' }, output: health }])
  await second
  assert.deepEqual(seen, ['running'])
  releaseFirst([{ terminalId: 'session', status: { state: 'exited', exitCode: 1 }, output: health }])
  await first
  await third
  assert.deepEqual(seen, ['running'])
})
