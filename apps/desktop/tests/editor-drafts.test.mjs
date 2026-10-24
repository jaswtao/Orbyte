/** Recovery queue concurrency regressions. Author: jaswtao · Date: 2026-10-24 */
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import ts from 'typescript'
const source = await readFile(new URL('../src/services/editor/drafts.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } })
const { DraftSync } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
const deferred = () => { let resolve; const promise = new Promise((done) => { resolve = done }); return { promise, resolve } }

test('late old writes cannot overtake new input or save cleanup', async () => {
  let snapshot = { content: 'old', revision: '1' }, stored = null
  const gate = deferred(), writes = []
  const sync = new DraftSync(() => snapshot, async (draft) => { writes.push(draft.revision); if (draft.revision === '1') await gate.promise; stored = draft }, async (draft) => { if (stored?.revision === draft.revision) stored = null }, assert.fail)
  const first = sync.flush()
  await Promise.resolve()
  snapshot = null; const clean = sync.flush()
  snapshot = { content: 'typed during save', revision: '2' }; const latest = sync.flush()
  gate.resolve(); await Promise.all([first, clean, latest])
  assert.deepEqual(writes, ['1', '2']); assert.equal(stored.content, 'typed during save')
  await sync.flush(); assert.deepEqual(writes, ['1', '2'])
  sync.dispose()
})
test('discard waits for inflight write and removal cannot recreate the discarded buffer', async () => {
  const gate = deferred(); let stored = null
  const sync = new DraftSync(() => ({ content: 'discarded', revision: '1' }), async (d) => { await gate.promise; stored = d }, async () => { stored = null }, assert.fail)
  const writing = sync.flush(); await Promise.resolve()
  const discarded = sync.discard(); gate.resolve(); await Promise.all([writing, discarded])
  await sync.flush(); assert.equal(stored, null); sync.dispose()
})
test('failed writes preserve the last successful revision and permit retry', async () => {
  let fail = true, stored = { content: 'old', revision: 'old' }, errors = 0
  const sync = new DraftSync(() => ({ content: 'new', revision: 'new' }), async (d) => { if (fail) throw Error('disk full'); stored = d }, async () => { stored = null }, () => errors++, stored)
  await assert.rejects(sync.flush()); assert.equal(stored.content, 'old')
  fail = false; await sync.flush(); assert.equal(stored.content, 'new'); assert.equal(errors, 1)
  await sync.discard(); assert.equal(stored, null); sync.dispose()
})

test('retry flush completes a failed discard without recreating the dirty buffer', async () => {
  let fail = true, attempts = 0, stored = { content: 'unsaved', revision: '1' }
  const sync = new DraftSync(() => ({ content: 'unsaved', revision: '1' }), assert.fail, async () => {
    attempts++
    if (fail) throw Error('temporary storage failure')
    stored = null
  }, () => {}, stored)
  await assert.rejects(sync.discard())
  assert.ok(stored)
  fail = false
  await sync.flush()
  assert.equal(attempts, 2); assert.equal(stored, null)
  await sync.flush(); assert.equal(stored, null)
  sync.dispose()
})

test('deleting a listed revision waits for writes and preserves a newer revision', async () => {
  const old = { content: 'old', revision: '1' }, gate = deferred()
  let stored = old
  const latest = { content: 'new', revision: '2' }
  const sync = new DraftSync(() => latest, async (d) => { await gate.promise; stored = d }, async (d) => {
    if (stored?.revision === d.revision) stored = null
  }, assert.fail, old)
  const writing = sync.flush()
  const deleting = sync.deleteDraft(old)
  gate.resolve(); await Promise.all([writing, deleting])
  assert.equal(stored, latest)
  await sync.flush(); assert.equal(stored, latest)
  sync.dispose()
})
test('continuous input flushes at the maximum interval; undo cleans the draft', async (context) => {
  context.mock.timers.enable({ apis: ['setTimeout'] })
  let snapshot = { content: 'input', revision: '1' }, stored = null
  const sync = new DraftSync(() => snapshot, async (d) => { stored = d }, async () => { stored = null }, assert.fail, null, 750, 5000)
  for (let i = 0; i < 10; i++) { sync.schedule(); context.mock.timers.tick(500) }
  await sync.flush(); assert.equal(stored.content, 'input')
  snapshot = null; sync.schedule(); context.mock.timers.tick(750); await sync.flush()
  assert.equal(stored, null); sync.dispose()
})
