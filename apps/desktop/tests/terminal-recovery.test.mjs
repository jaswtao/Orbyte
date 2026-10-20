/** Output acknowledgement and failure recovery regression tests. Author: jaswtao · Date: 2026-10-20 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { Channel } from '@tauri-apps/api/core'

const { OutputRecovery, AckQueue } = await loadService('terminal/recovery')
const flush = () => new Promise(setImmediate)
const { InputQueue } = await loadService('terminal/inputQueue')
const frameModule = await loadService('terminal/frame')
const recoveryModule = await loadService('terminal/recovery')

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

function clientFixture(t) {
  const callbacks = new Map()
  let next = 0
  const previous = globalThis.window
  const window = globalThis.window = { setTimeout, clearTimeout, __TAURI_INTERNALS__: {
    transformCallback(callback) { callbacks.set(++next, callback); return next }, unregisterCallback(id) { callbacks.delete(id) },
  } }
  const statuses = []
  const diagnostics = []
  const api = { reportDiagnostic: async (...args) => diagnostics.push(args), ackTerminal: async () => {}, resizeTerminal: async () => {}, writeTerminal: async () => {} }
  class Terminal {
    options = {}
    buffer = { active: {} }
    parser = { registerOscHandler() {} }
    loadAddon() {} open() {} onResize() {} onWriteParsed() {} onData(callback) { this.data = callback }
    write(data, callback) { if (this.failWrite) throw new Error('parser failed'); callback?.() }
    dispose() {} reset() {}
  }
  const exports = {}
  vm.runInNewContext(clientSource, { exports, window, document: { visibilityState: 'visible' }, performance, queueMicrotask, ArrayBuffer,
    ResizeObserver: class { observe() {} disconnect() {} },
    require(name) {
      if (name === '@tauri-apps/api/core') return { Channel }
      if (name === '@xterm/addon-fit') return { FitAddon: class { fit() {} proposeDimensions() { return { cols: 80, rows: 24 } } } }
      if (name === '@xterm/xterm') return { Terminal }
      if (name === '../terminalApi') return { terminalApi: api }
      if (name === './recovery') return recoveryModule
      if (name === './frame') return frameModule
      if (name === './inputQueue') return { InputQueue }
      if (name === './ime') return { bindTerminalIme: () => ({ dispose() {}, reset() {} }) }
      if (name === './selection') return { bindTerminalSelection: () => ({ dispose() {} }) }
      if (name === './shellIntegration') return { ShellIntegration: class { reset() {} input() {} } }
      if (name === './contentActivity') return { ContentActivity: class { reset() {} resized() {} } }
      if (name === '../../i18n/translate') return { t: (key) => key }
      throw new Error(name)
    },
  })
  const container = { dataset: {}, offsetWidth: 0, offsetHeight: 0, contains: () => false }
  const client = new exports.TerminalClient(container, {}, { onStatus: (status) => statuses.push(status) })
  const channel = client.createChannel()
  client.attach('session')
  let index = 0
  function deliver(kind, payload) {
    const data = typeof payload === 'string' ? new TextEncoder().encode(payload) : payload
    const frame = new ArrayBuffer(17 + data.length)
    new DataView(frame).setUint8(0, kind)
    new DataView(frame).setBigUint64(1, BigInt(index), true)
    new Uint8Array(frame, 17).set(data)
    callbacks.get(channel.id)({ index: index++, message: frame })
  }
  t.after(() => { client.dispose(); globalThis.window = previous })
  return { client, deliver, statuses, diagnostics, api, callbacks, channel, container }
}

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
  assert.deepEqual(nudges.filter((item) => item[3] === true), [['session', 80, 24, true]])
  assert.ok(f.diagnostics.some((args) => args[1] === 'reconnect_succeeded'))
  newest.onmessage(frame(2, '{bad again'))
  await flush()
  assert.equal(subscriptions, 1)
  assert.equal(f.client.connection, 'disconnected')
  assert.equal(f.client.term.options.disableStdin, true)
  await f.client.reconnect()
  assert.equal(subscriptions, 2)
  assert.equal(f.client.id, 'session')
  assert.equal(f.client.term.options.disableStdin, false)
  assert.deepEqual(nudges.filter((item) => item[3] === true), [['session', 80, 24, true], ['session', 80, 24, true]])
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
  assert.equal(f.client.term.options.disableStdin, true)

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
  assert.equal(f.client.term.options.disableStdin, true)
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
