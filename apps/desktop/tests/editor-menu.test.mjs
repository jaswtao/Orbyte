/** Editor context menu behavior. Author: jaswtao · Date: 2026-10-30 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { computed, ref, shallowReactive, watch } from 'vue'
import { EditorSelection, EditorState, Transaction } from '@codemirror/state'
import { history, redo, redoDepth, selectAll, undo, undoDepth } from '@codemirror/commands'

const source = await readFile(new URL('../src/components/editor/EditorPane.vue', import.meta.url), 'utf8')
const script = source.split('<script setup lang="ts">')[1].split('</script>')[0].replace(/^import .*$/gm, '')
const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
function fixture() {
  const calls = [], errors = [], lifecycle = {}, events = [], panels = []
  let focused = 0
  const view = {
    state: EditorState.create({ doc: 'hello world', selection: { anchor: 0, head: 5 }, extensions: [history()] }),
    dispatch(transaction) { this.state = transaction instanceof Transaction ? transaction.state : this.state.update(transaction).state },
    focus() { focused++ },
    posAtCoords: () => 8,
    dom: { querySelector: () => ({ focus: () => panels.push('replace') }) },
  }
  // CodeMirror commands accept an unbound dispatch function.
  view.dispatch = view.dispatch.bind(view)
  const entry = { view, busy: false, initialized: true, metadata: {}, recovery: null }
  const props = shallowReactive({ id: 'file', visible: true, focused: true, editors: { entries: shallowReactive(new Map([['file', entry]])), save: async (id) => calls.push(['save', id]) } })
  const context = {
    Element: class { closest() { return null } },
    computed, ref, watch, Transaction, undo, redo, undoDepth, redoDepth, selectAll,
    defineProps: () => props, defineEmits: () => (...args) => events.push(args),
    onMounted: (fn) => { lifecycle.mount = fn }, onBeforeUnmount: (fn) => { lifecycle.unmount = fn },
    openSearchPanel: () => panels.push('find'), showWorkspaceMessage: (error) => errors.push(error),
    document: { addEventListener() {}, removeEventListener() {} },
    t: (key) => key,
    invoke: async (...args) => { calls.push(args); if (context.fail) throw new Error('clipboard unavailable'); return context.clipboard ?? 'pasted' },
  }
  vm.createContext(context)
  vm.runInContext(js, context)
  return { view, entry, props, calls, errors, context, lifecycle, panels, events, focused: () => focused, run: (code) => vm.runInContext(code, context) }
}

test('Right click preserves selection, locates an empty cursor, and opens at viewport coordinates', () => {
  const f = fixture()
  f.run('openMenu({ clientX: 120, clientY: 240, preventDefault() {}, stopPropagation() {} })')
  assert.equal(f.view.state.selection.main.from, 0)
  assert.equal(f.view.state.selection.main.to, 5)
  assert.equal(f.run('menu.value.x'), 120)
  assert.equal(f.run('menu.value.y'), 240)
  f.view.dispatch({ selection: { anchor: 1 } })
  f.run('openMenu({ clientX: 120, clientY: 240, preventDefault() {}, stopPropagation() {} })')
  assert.equal(f.view.state.selection.main.head, 8)
  assert.equal(f.focused(), 2)
  assert.deepEqual(f.events[0], ['focus', 'file'])
  assert.match(source, /@mousedown.capture="preserveSelection" @contextmenu="openMenu" @scroll.capture="closeMenu"/)
  let prevented = 0, stopped = 0
  f.context.mouse = { target: new f.context.Element(), button: 2, clientX: 120, clientY: 240, preventDefault: () => prevented++, stopPropagation: () => stopped++ }
  f.run('preserveSelection(mouse); openMenu(mouse)')
  f.context.mouse.button = 0
  f.run('preserveSelection(mouse)')
  assert.equal(prevented, 2)
  assert.equal(stopped, 1)
  f.props.focused = false
  f.run('openMenu({ clientX: 120, clientY: 240, preventDefault() {}, stopPropagation() {} })')
  f.props.focused = true
  assert.equal(f.run('menu.value.x'), 120)
  f.props.focused = false
  assert.equal(f.run('menu.value'), null)
})

test('Search and replace fields keep native right clicks without changing document selection or focus', () => {
  for (const field of ['search', 'replace']) for (const selected of [true, false]) {
    const f = fixture()
    if (!selected) f.view.dispatch({ selection: { anchor: 1 } })
    const state = f.view.state
    let prevented = 0, stopped = 0
    const target = new f.context.Element()
    target.closest = (selector) => selector === '.cm-panels' ? { field } : null
    f.context.mouse = {
      target, button: 2, clientX: 120, clientY: 240,
      preventDefault: () => prevented++, stopPropagation: () => stopped++,
    }
    f.run('preserveSelection(mouse); openMenu(mouse)')
    assert.equal(prevented, 0)
    assert.equal(stopped, 0)
    assert.equal(f.run('menu.value'), null)
    assert.equal(f.view.state, state)
    assert.equal(f.focused(), 0)
    assert.deepEqual(f.events, [])
    assert.deepEqual(f.calls, [])
  }
})

test('Menu enables history only when available and protects read-only, recovering and busy editors', async () => {
  const f = fixture()
  assert.equal(f.run("menuOptions().find(o => o.key === 'undo').disabled"), true)
  f.view.dispatch({ changes: { from: 0, to: 5, insert: 'changed' } })
  assert.equal(f.run("menuOptions().find(o => o.key === 'undo').disabled"), false)
  await f.run("selectMenu('undo')")
  assert.equal(f.view.state.doc.toString(), 'hello world')
  await f.run("selectMenu('redo')")
  assert.equal(f.view.state.doc.toString(), 'changed world')
  for (const condition of ['readOnly', 'busy', 'recovery', 'uninitialized']) {
    const g = fixture()
    if (condition === 'readOnly') g.view.state = EditorState.create({ doc: 'hello world', selection: { anchor: 0, head: 5 }, extensions: [EditorState.readOnly.of(true)] })
    if (condition === 'busy') g.entry.busy = true
    if (condition === 'recovery') g.entry.recovery = {}
    if (condition === 'uninitialized') g.entry.initialized = false
    for (const key of ['cut', 'paste', 'replace', 'undo', 'redo']) {
      assert.equal(g.run(`menuOptions().find(o => o.key === '${key}').disabled`), true)
      await g.run(`selectMenu('${key}')`)
    }
    assert.equal(g.run("menuOptions().find(o => o.key === 'copy').disabled"), false)
    assert.deepEqual(g.calls, [])
  }
  f.view.dispatch({ selection: { anchor: 0 } })
  for (const key of ['cut', 'copy']) assert.equal(f.run(`menuOptions().find(o => o.key === '${key}').disabled`), true)
})

test('Clipboard copy, cut and paste preserve undo history; failed cut keeps original text', async () => {
  const f = fixture()
  await f.run("selectMenu('copy')")
  assert.equal(f.calls[0][1].text, 'hello')
  assert.equal(f.view.state.doc.toString(), 'hello world')
  f.context.fail = true
  await f.run("selectMenu('cut')")
  assert.equal(f.view.state.doc.toString(), 'hello world')
  assert.deepEqual(f.errors, ['editor.clipboardFailed'])
  f.context.fail = false
  await f.run("selectMenu('cut')")
  assert.equal(f.view.state.doc.toString(), ' world')
  undo(f.view)
  assert.equal(f.view.state.doc.toString(), 'hello world')
  f.context.clipboard = 'line1\nline2'
  await f.run("selectMenu('paste')")
  assert.equal(f.view.state.doc.toString(), 'line1\nline2 world')
  undo(f.view)
  assert.equal(f.view.state.doc.toString(), 'hello world')
  f.context.clipboard = ''
  await f.run("selectMenu('paste')")
  assert.equal(f.view.state.doc.toString(), 'hello world')
})

test('Multiple selections copy and cut the same ranges', async () => {
  const f = fixture()
  f.view.state = EditorState.create({ doc: 'hello world', selection: EditorSelection.create([EditorSelection.range(0, 5), EditorSelection.range(6, 11)]), extensions: [EditorState.allowMultipleSelections.of(true), history()] })
  await f.run("selectMenu('cut')")
  assert.equal(f.calls[0][1].text, 'hello\nworld')
  assert.equal(f.view.state.doc.toString(), ' ')
  undo(f.view)
  assert.equal(f.view.state.doc.toString(), 'hello world')
})

test('Pending clipboard work cannot change a closed, hidden, changed or newly read-only editor', async () => {
  for (const action of ['cut', 'paste']) for (const change of ['closed', 'hidden', 'selection', 'content', 'readonly', 'busy', 'unmounted', 'switchedBack']) {
    const f = fixture()
    let resolve
    f.context.invoke = () => new Promise((done) => { resolve = done })
    const pending = f.run(`selectMenu('${action}')`)
    assert.equal(f.run("menuOptions().find(o => o.key === 'paste').disabled"), true)
    if (change === 'closed') f.props.editors.entries.delete('file')
    if (change === 'hidden') f.props.visible = false
    if (change === 'selection') f.view.dispatch({ selection: { anchor: 6 } })
    if (change === 'content') f.view.dispatch({ changes: { from: 11, insert: '!' } })
    if (change === 'readonly') f.view.state = EditorState.create({ doc: 'hello world', extensions: [EditorState.readOnly.of(true)] })
    if (change === 'busy') f.entry.busy = true
    if (change === 'unmounted') f.lifecycle.unmount()
    if (change === 'switchedBack') { f.props.visible = false; f.props.visible = true }
    const before = f.view.state.doc.toString()
    resolve('replacement')
    await pending
    assert.equal(f.view.state.doc.toString(), before, `${action}: ${change}`)
  }
})

test('Select all, find, replace and save use the clicked editor, while Escape dismisses and restores focus', async () => {
  const f = fixture()
  await f.run("selectMenu('selectAll')")
  assert.equal(f.view.state.selection.main.to, 11)
  await f.run("selectMenu('find')")
  await f.run("selectMenu('replace')")
  assert.deepEqual(f.panels, ['find', 'find', 'replace'])
  await f.run("selectMenu('save')")
  assert.deepEqual(f.calls, [['save', 'file']])
  f.entry.metadata = null
  await f.run("selectMenu('save')")
  assert.equal(f.calls.length, 1)
  f.run('openMenu({ clientX: 1, clientY: 2, preventDefault() {}, stopPropagation() {} })')
  let prevented = false, stopped = false
  f.context.escape = { key: 'Escape', preventDefault: () => { prevented = true }, stopPropagation: () => { stopped = true } }
  f.run('escapeMenu(escape)')
  assert.equal(f.run('menu.value'), null)
  assert.equal(prevented && stopped, true)
  assert.match(source, /@clickoutside="closeMenu"/)
  f.run('openMenu({ clientX: 1, clientY: 2, preventDefault() {}, stopPropagation() {} })')
  f.props.visible = false
  assert.equal(f.run('menu.value'), null)
})
