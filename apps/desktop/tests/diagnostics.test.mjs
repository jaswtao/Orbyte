/** Diagnostic transport/privacy regression tests. Author: jaswtao · Date: 2026-10-23 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'

const source = ts.transpileModule(await readFile(new URL('../src/services/diagnostics.ts', import.meta.url), 'utf8'), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText
function fixture(run = async () => 'result') {
  const records = []
  const listeners = new Map()
  const exports = {}
  let now = 0
  vm.runInNewContext(source, { exports, crypto: { randomUUID: () => 'page' }, Error,
    performance: { now: () => now }, window: { addEventListener: (event, callback) => listeners.set(event, callback) },
    require: () => ({ invoke: async (command, args, options) => {
      if (command === 'report_app_diagnostic') { records.push(args); return }
      return run(command, args, options)
    } }),
  })
  return { ...exports, records, listeners, clock: (value) => { now = value } }
}

test('commands preserve results and correlate metadata without payloads', async () => {
  const f = fixture(async (command, args) => { assert.equal(command, 'write_file'); assert.equal(args.content, 'SECRET'); return 42 })
  const id = '12345678-1234-1234-1234-123456789abc'
  assert.equal(await f.invoke('write_file', { projectId: id, content: 'SECRET', path: 'PRIVATE', request: { content: 'HIDDEN' } }), 42)
  assert.equal(f.records.length, 2)
  assert.equal(f.records[0].event, 'command_started')
  assert.equal(f.records[1].outcome, 'ok')
  assert.equal(f.records[0].context, f.records[1].context)
  assert.ok(f.records[1].context.includes(id))
  const serialized = JSON.stringify(f.records)
  for (const text of ['SECRET', 'PRIVATE', 'HIDDEN']) assert.ok(!serialized.includes(text))
})

test('failures preserve the original rejection and only log its category', async () => {
  const error = { kind: 'unmappable', message: 'SECRET', position: 1 }
  const f = fixture(async () => { throw error })
  await assert.rejects(f.invoke('write_file'), (actual) => actual === error)
  assert.equal(f.records.at(-1).outcome, 'error')
  assert.match(f.records.at(-1).context, /kind=unmappable/)
  assert.ok(!JSON.stringify(f.records).includes('SECRET'))
})

test('high-frequency successes are quiet but slow calls and failures are recorded', async () => {
  let fail = false
  const f = fixture(async () => { if (fail) throw 'SECRET'; f.clock(1500); return null })
  f.clock(1500)
  await f.invoke('ack_terminal')
  assert.equal(f.records.length, 0)
  f.clock(0)
  await f.invoke('ack_terminal')
  assert.equal(f.records[0].elapsedMs, 1500)
  fail = true
  await assert.rejects(f.invoke('write_terminal', { data: 'PRIVATE' }))
  assert.equal(f.records.at(-1).outcome, 'error')
  assert.ok(!JSON.stringify(f.records).includes('PRIVATE'))
})

test('diagnostic transport failure never recurses or changes command results', async () => {
  const exports = {}
  let reports = 0
  vm.runInNewContext(source, { exports, crypto: { randomUUID: () => 'page' }, performance,
    require: () => ({ invoke: async (command) => {
      if (command === 'report_app_diagnostic') { reports++; throw new Error('offline') }
      return 7
    } }),
  })
  assert.equal(await exports.invoke('create_group'), 7)
  await new Promise(setImmediate)
  assert.equal(reports, 2)
})

test('global and Vue errors keep source coordinates without messages or rejection payloads', () => {
  const f = fixture()
  f.installDiagnostics()
  const error = new Error('SECRET')
  error.stack = 'Error: SECRET\n at handler (http://localhost/assets/index-abc.js:10:2)'
  f.listeners.get('error')({ lineno: 10, colno: 2, error })
  f.listeners.get('unhandledrejection')({ reason: 'PRIVATE' })
  const app = { config: {} }
  assert.equal(f.attachVueDiagnostics(app), app)
  app.config.errorHandler(error, null, 'render function')
  assert.deepEqual(f.records.map((r) => r.event), ['frontend_ready', 'window_error', 'unhandled_rejection', 'vue_error'])
  assert.ok(f.records.at(-1).context.includes('index-abc.js:10:2'))
  for (const text of ['SECRET', 'PRIVATE', 'http://localhost']) assert.ok(!JSON.stringify(f.records).includes(text))
})
