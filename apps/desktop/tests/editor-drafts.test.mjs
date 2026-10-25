/** Recovery queue concurrency regressions. Author: jaswtao · Date: 2026-10-24 */
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import ts from 'typescript'
import vm from 'node:vm'
const source = await readFile(new URL('../src/services/editor/drafts.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } })
const { DraftSync } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
const deferred = () => { let resolve; const promise = new Promise((done) => { resolve = done }); return { promise, resolve } }

const workspaceComponent = await readFile(new URL('../src/components/workspace/WorkspaceView.vue', import.meta.url), 'utf8')
const workspaceScript = workspaceComponent.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1]
const workspaceAst = ts.createSourceFile('WorkspaceView.ts', workspaceScript, ts.ScriptTarget.Latest, true)
const menuScript = workspaceAst.statements.filter((node) =>
  (ts.isFunctionDeclaration(node) && node.name?.text === 'onAddSelect') ||
  (ts.isVariableStatement(node) && node.declarationList.declarations.some((declaration) => declaration.name.getText(workspaceAst) === 'addMenu')),
).map((node) => node.getText(workspaceAst)).join('\n')
const menuCode = ts.transpileModule(menuScript, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

test('draft recovery remains available before workspace readiness while mutations stay blocked', () => {
  const dropdown = workspaceComponent.match(/<NDropdown trigger="click"[^>]*@select="onAddSelect"[^>]*>/)[0]
  const button = workspaceComponent.match(/<button class="workspace-tab-add"[^>]*>/)[0]
  assert.doesNotMatch(dropdown, /:disabled=/)
  assert.doesNotMatch(button, /:disabled=/)
  for (const busy of [false, true]) for (const ready of [false, true]) {
    const events = [], actions = []
    const context = vm.createContext({
      busy: { value: busy }, ready: { value: ready },
      activeTab: { value: {} }, focused: { value: 'pane' },
      shells: { value: [{ shellType: 'powershell', available: true }] },
      presets: { value: [{ preset: 'codex', available: true }] },
      computed: (get) => ({ get value() { return get() } }),
      t: (key) => key, hint: () => '', shellLabel: () => '', presetLabel: () => '',
      emit: (...event) => events.push(event),
      chooseFile: () => actions.push('file'), create: () => actions.push('terminal'),
    })
    vm.runInContext(menuCode, context)
    vm.runInContext("onAddSelect('recover-drafts')", context)
    assert.deepEqual(events, [['recoverDrafts']])
    assert.equal(vm.runInContext("!!addMenu.value.find((option) => option.key === 'recover-drafts').disabled", context), false)
    for (const key of ['powershell', 'codex', 'open-file', 'open-file-horizontal', 'open-file-vertical']) {
      assert.equal(vm.runInContext(`!!addMenu.value.flatMap((option) => option.children ?? [option]).find((option) => option.key === '${key}').disabled`, context), busy || !ready)
      vm.runInContext(`onAddSelect('${key}')`, context)
    }
    assert.equal(actions.length, busy || !ready ? 0 : 5)
  }
})

const recoveryComponent = await readFile(new URL('../src/components/editor/DraftRecovery.vue', import.meta.url), 'utf8')
const recoveryScript = recoveryComponent.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1].replace(/^import .*$/gm, '')
const recoveryCode = ts.transpileModule(recoveryScript, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText
function recoveryFixture(load) {
  let mounted
  const events = [], deleted = []
  const context = vm.createContext({
    defineProps: () => ({ projects: [], editors: { async deleteDraft(draft) { deleted.push(draft) } } }),
    defineEmits: () => (...event) => events.push(event),
    defineExpose() {},
    ref: (value) => ({ value }), watch() {}, onMounted: (callback) => { mounted = callback },
    fileApi: { drafts: load }, t: (key) => key,
  })
  vm.runInContext(recoveryCode, context)
  return { mount: () => mounted(), run: (code) => vm.runInContext(code, context), events, deleted }
}

test('startup waits for draft detection and only prompts when drafts exist', async () => {
  for (const drafts of [[], [{ projectId: 'p', path: 'unsaved.txt', content: 'draft' }]]) {
    const gate = deferred(), f = recoveryFixture(() => gate.promise)
    const mounting = f.mount()
    assert.equal(f.run('show.value'), false)
    gate.resolve(drafts); await mounting
    assert.equal(f.run('show.value'), drafts.length > 0)
    assert.equal(f.run('drafts.value'), drafts)
    assert.deepEqual(f.events, [])
  }
})

test('skipping recovery closes the prompt without deleting drafts', async () => {
  const drafts = [{ projectId: 'p', path: 'unsaved.txt', content: 'draft' }]
  const f = recoveryFixture(async () => drafts)
  await f.mount()
  const skip = recoveryComponent.match(/<NButton @click="([^"]+)">\{\{ t\('draft.skip'\)/)[1]
  f.run(skip.replace(/\bshow\b/g, 'show.value'))
  assert.equal(f.run('show.value'), false)
  assert.equal(f.run('drafts.value'), drafts)
  assert.deepEqual(f.deleted, [])
})

test('failed startup detection reports an error without opening an empty prompt', async () => {
  const f = recoveryFixture(async () => { throw Error('storage unavailable') })
  await f.mount()
  assert.equal(f.run('show.value'), false)
  assert.deepEqual(f.events, [['error', 'draft.failed']])
})

test('reopening recovery after skipping refreshes drafts and clears the previous preview', async () => {
  const drafts = [{ projectId: 'p', path: 'deleted.txt', content: 'draft' }]
  let stored = drafts
  const f = recoveryFixture(async () => stored)
  await f.mount()
  f.run('selected.value = drafts.value[0]; show.value = false')
  stored = [{ projectId: 'p', path: 'another.txt', content: 'new draft' }]
  await f.run('open()')
  assert.equal(f.run('show.value'), true)
  assert.equal(f.run('drafts.value'), stored)
  assert.equal(f.run('selected.value'), null)
  f.run('show.value = false')
  stored = []
  await f.run('open()')
  assert.equal(f.run('show.value'), true)
  assert.equal(f.run('drafts.value'), stored)
  assert.deepEqual(f.deleted, [])
})

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
