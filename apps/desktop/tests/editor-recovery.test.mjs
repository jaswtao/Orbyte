/** Registry recovery and save lifecycle regressions. Author: jaswtao · Date: 2026-10-24 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { Compartment, EditorState, Prec } from '@codemirror/state'
import { EditorView as RealView } from '@codemirror/view'

async function code(path) {
  const text = (await readFile(new URL(`../src/services/editor/${path}.ts`, import.meta.url), 'utf8')).replace(/^import .*$/gm, '')
  return ts.transpileModule(text, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText
}
const compiled = [await code('document'), await code('drafts'), await code('registry')].join('\n')
class TestView {
  static theme() { return [] }
  static domEventHandlers() { return [] }
  static updateListener = RealView.updateListener
  constructor({ state }) { this.state = state }
  setState(state) { this.state = state }
  dispatch(spec) {
    const transaction = this.state.update(spec); this.state = transaction.state
    for (const listener of this.state.facet(RealView.updateListener)) listener({ state: this.state, docChanged: transaction.docChanged })
  }
  destroy() {}
}
const tick = () => new Promise(setImmediate)
async function fixture({ missing = false, changed = false, rootChanged = false } = {}) {
  const draft = { projectId: 'p', projectPath: 'C:/original', path: 'file.txt', content: 'recovered text', baseline: 'v1', encoding: 'utf-8', bom: false, lineEnding: 'LF', revision: 'previous-session', updatedAt: '' }
  const disk = { content: changed ? 'external' : 'disk', version: changed ? 'v2' : 'v1', encoding: 'utf-8', bom: false, lineEnding: 'LF' }
  let stored = { ...draft }, writing, draftWriting, deleting
  const errors = [], writes = [], deletions = []
  const fileApi = {
    async unsaved() {}, async read() { if (missing) throw { kind: 'missing', message: 'missing' }; return disk },
    async drafts() { return stored ? [stored] : [] }, async putDraft(d) { if (draftWriting) await draftWriting; stored = { ...d } },
    async deleteDraft(project, path, revision) { if (deleting) await deleting; deletions.push(revision); if (stored?.revision === revision) stored = null },
    async write(project, path, request) {
      writes.push(request)
      if (writing) await writing
      if (changed && request.expectedVersion !== null) throw { kind: 'conflict', message: 'conflict' }
      return { ...disk, content: request.content, version: 'saved' }
    },
    async renameDrafts(project, from, to) { if (stored) stored.path = to + stored.path.slice(from.length) },
  }
  const exports = {}
  const context = vm.createContext({ exports, fileApi, fileError: (e) => e, DraftSync: undefined, EditorView: TestView, Compartment, EditorState, Prec, basicSetup: [],
    shallowReactive: (value) => value, markRaw: (value) => value, unsavedCount: undefined,
    palettes: { dark: {} }, resolveAppearance: () => 'dark', syntaxHighlighting: () => [], HighlightStyle: { define: () => [] }, tags: new Proxy({}, { get: () => () => {} }),
    t: (key) => key, matchShortcut: () => false, window: { matchMedia: () => ({ matches: false }) }, crypto, setTimeout, clearTimeout,
  })
  // Classes compiled in one lexical scope let the registry use the real queue and document model.
  vm.runInContext(compiled, context)
  const registry = new exports.EditorRegistry((e) => errors.push(e), () => ({}), () => false, () => rootChanged ? 'C:/new' : 'C:/original')
  const entry = registry.create('editor', 'p', 'file.txt', {})
  for (let i = 0; i < 10 && entry.busy; i++) await tick()
  return { registry, entry, errors, writes, deletions, stored: () => stored, delayDelete: (p) => { deleting = p }, delayDraft: (p) => { draftWriting = p }, delayWrite: (p) => { writing = p } }
}

test('initialization keeps the disk buffer read-only until a draft decision; restore never writes disk', async () => {
  const f = await fixture()
  assert.equal(f.entry.view.state.readOnly, true); assert.equal(f.entry.document.content, 'disk')
  assert.equal(f.stored().content, 'recovered text')
  await f.registry.recover('editor', true)
  assert.equal(f.entry.view.state.readOnly, false); assert.equal(f.entry.document.content, 'recovered text')
  assert.equal(f.entry.document.dirty, true); assert.equal(f.writes.length, 0)
  await f.registry.save('editor')
  assert.equal(f.entry.document.dirty, false); assert.equal(f.stored(), null)
  f.registry.remove('editor')
})
test('changed disk preserves the original conflict version; missing files retain recoverable text', async () => {
  for (const options of [{ changed: true }, { missing: true }]) {
    const f = await fixture(options); await f.registry.recover('editor', true)
    assert.equal(f.entry.document.version, 'v1'); assert.equal(f.entry.document.content, 'recovered text')
    if (options.changed) { assert.equal(f.entry.document.external, true); await f.registry.save('editor'); assert.equal(f.entry.dialog.kind, 'conflict'); assert.ok(f.stored()) }
    else assert.equal(f.entry.deleted, true)
    await f.registry.discard('editor'); f.registry.remove('editor'); await tick(); assert.equal(f.stored(), null)
  }
})
test('new input while saving remains dirty and replaces the saved revision with a new draft', async () => {
  const f = await fixture(); await f.registry.recover('editor', true)
  let release; f.delayWrite(new Promise((done) => { release = done }))
  const saving = f.registry.save('editor')
  f.entry.view.dispatch({ changes: { from: 0, to: f.entry.view.state.doc.length, insert: 'later input' } })
  release(); await saving
  assert.equal(f.entry.document.dirty, true); assert.equal(f.stored().content, 'later input'); assert.equal(f.stored().baseline, 'saved')
  await f.registry.discard('editor'); f.registry.remove('editor')
})
test('project root changes block restore but allow explicit discard', async () => {
  const f = await fixture({ rootChanged: true }); await f.registry.recover('editor', true)
  assert.ok(f.entry.recovery); assert.equal(f.entry.document.content, 'disk')
  await f.registry.recover('editor', false)
  assert.equal(f.stored(), null); assert.equal(f.entry.view.state.readOnly, false); f.registry.remove('editor')
})
test('directory rename migrates persisted copies and subsequent edits use the new path', async () => {
  const f = await fixture(); await f.registry.recover('editor', true)
  await f.registry.rename('p', 'file.txt', 'renamed.txt')
  assert.equal(f.entry.path, 'renamed.txt'); assert.equal(f.stored().path, 'renamed.txt')
  f.entry.view.dispatch({ changes: { from: 0, insert: 'new ' } }); await f.entry.draftSync.flush()
  assert.equal(f.stored().content, 'new recovered text')
  await f.registry.discard('editor'); assert.equal(f.stored(), null); f.registry.remove('editor')
})

test('rapid close and reopen waits for the previous buffer to persist before offering recovery', async () => {
  const f = await fixture(); await f.registry.recover('editor', true)
  f.entry.view.dispatch({ changes: { from: 0, insert: 'latest ' } })
  let release; f.delayDraft(new Promise((done) => { release = done }))
  f.registry.remove('editor')
  const reopened = f.registry.create('new-editor', 'p', 'file.txt', {})
  await tick(); assert.equal(reopened.initialized, false)
  release(); for (let i = 0; i < 10 && reopened.busy; i++) await tick()
  assert.equal(reopened.recovery.content, 'latest recovered text')
  await f.registry.recover('new-editor', false); f.registry.remove('new-editor')
})

test('undoing a restored buffer to its known disk baseline clears the persisted draft', async () => {
  const f = await fixture(); await f.registry.recover('editor', true)
  f.entry.view.dispatch({ changes: { from: 0, to: f.entry.view.state.doc.length, insert: 'disk' } })
  assert.equal(f.entry.document.dirty, false); await f.entry.draftSync.flush()
  assert.equal(f.stored(), null); f.registry.remove('editor')
})

test('explicit reload keeps input locked until draft cleanup finishes', async () => {
  const f = await fixture(); await f.registry.recover('editor', true)
  let release; f.delayDelete(new Promise((done) => { release = done }))
  const loading = f.registry.reload('editor')
  await tick(); assert.equal(f.entry.view.state.readOnly, true)
  release(); await loading
  assert.equal(f.entry.view.state.readOnly, false); assert.equal(f.entry.document.content, 'disk')
  assert.equal(f.stored(), null); f.registry.remove('editor')
})

test('deleting an active draft allows the unchanged dirty buffer to persist again', async () => {
  const f = await fixture(); await f.registry.recover('editor', true)
  const listed = { ...f.stored() }
  await f.registry.deleteDraft(listed)
  assert.equal(f.stored(), null)
  assert.equal(f.entry.document.dirty, true)
  assert.equal(f.entry.document.content, listed.content)
  await f.entry.draftSync.flush()
  assert.equal(f.stored().content, listed.content)
  assert.notEqual(f.stored().revision, listed.revision)
  await f.registry.discard('editor'); f.registry.remove('editor')
})

test('deleting a pending recovery draft also unlocks its editor', async () => {
  const f = await fixture()
  await f.registry.deleteDraft({ ...f.stored() })
  assert.equal(f.stored(), null)
  assert.equal(f.entry.recovery, null)
  assert.equal(f.entry.view.state.readOnly, false)
  assert.equal(f.entry.document.content, 'disk')
  f.registry.remove('editor')
})
