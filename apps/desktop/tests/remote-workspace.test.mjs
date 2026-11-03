/** Remote workspace rules that do not need a server. Author: jaswtao · Date: 2026-10-31 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { computed, nextTick, watch, markRaw, shallowReactive, reactive, ref, effectScope } from 'vue'

test('remote project context menu renames and confirms deletion of the clicked project', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteSidebar.vue', import.meta.url), 'utf8')
  const script = source.split('<script setup lang="ts">')[1].split('</script>')[0].replace(/^import .*$/gm, '')
  const projects = [{ id: 'one', name: 'First' }, { id: 'two', name: 'Second' }]
  const saved = [], deleted = [], selected = [], errors = []
  const remote = {
    projects, activeProjectId: 'one', reload: async () => {},
    selectProject: (id) => selected.push(id),
  }
  const context = vm.createContext({
    computed, ref, onMounted() {}, defineExpose() {}, useRemote: () => remote,
    defineEmits: () => (_event, message) => errors.push(message), t: (key) => key,
    copyName: (name) => Array.from(`${name} copy`).slice(0, 100).join(''),
    remoteApi: {
      saveProject: async (...args) => { saved.push(args); return args[0] },
      deleteProject: async (id) => deleted.push(id),
    },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  const run = (code) => vm.runInContext(code, context)
  run('openMenu({ clientX: 120, clientY: 240 }, remote.projects[1])')
  assert.equal(run('contextMenu.value.x'), 120)
  assert.equal(run('contextMenu.value.y'), 240)
  assert.deepEqual(Array.from(run('menuOptions.value'), (item) => item.key), ['rename', 'duplicate', 'delete'])
  run("onMenuSelect('rename')")
  assert.equal(run('contextMenu.value'), null)
  assert.equal(run('projectDialog.value'), true)
  assert.equal(run('name.value'), 'Second')
  run("name.value = 'Renamed'")
  await run('save()')
  assert.deepEqual(saved, [['two', 'Renamed']])
  assert.deepEqual(selected, ['two'])
  run("openMenu({ clientX: 1, clientY: 2 }, remote.projects[0]); onMenuSelect('delete')")
  assert.equal(run('deleting.value.id'), 'one')
  assert.deepEqual(deleted, [])
  await run('remove()')
  assert.deepEqual(deleted, ['one'])
  assert.equal(run('deleting.value'), null)
  run('openMenu({ clientX: 1, clientY: 2 }, remote.projects[0]); busy.value = true')
  assert.ok(run('menuOptions.value.every((item) => item.disabled)'))
  run("onMenuSelect('delete')")
  assert.equal(run('deleting.value'), null)
  run('openMenu({ clientX: 1, clientY: 2 }, remote.projects[0])')
  assert.equal(run('contextMenu.value'), null)
  run('busy.value = false; openMenu({ clientX: 1, clientY: 2 }, remote.projects[1]); remote.projects.pop()')
  run("onMenuSelect('rename')")
  assert.equal(run('projectDialog.value'), false)
  remote.projects[0].name = 'N'.repeat(100)
  run('duplicate(remote.projects[0])')
  assert.equal(run('name.value'), 'N'.repeat(100))
  assert.equal(run('editing.value'), null)
  assert.equal(run('duplicating.value.id'), 'one')
  assert.deepEqual(errors, [])
  assert.match(source, /@contextmenu\.prevent\.stop="openMenu\(\$event, project\)"/)
  assert.match(source, /<ProjectContextMenu\b[^>]*@clickoutside="contextMenu = null"/)
})

test('duplicating an SSH connection pre-fills its settings without copying its saved password', async () => {
  const source = await readFile(new URL('../src/components/remote/ConnectionDialog.vue', import.meta.url), 'utf8')
  const script = source.split('<script setup lang="ts">')[1].split('</script>')[0].replace(/^import .*$/gm, '')
  const show = ref(false)
  const calls = []
  const connection = { id: 'source', name: 'Server', host: 'example.com', port: 22, username: 'root', authType: 'password', privateKeyPath: null, hasSavedPassword: true }
  const context = vm.createContext({
    computed, reactive, ref, watch, defineProps: () => ({ connection: null, duplicateSource: connection }),
    defineModel: () => show, defineEmits: () => () => {}, t: (key, params) => params ? `${params.name} copy` : key,
    copyName: (name) => Array.from(`${name} copy`).slice(0, 100).join(''),
    remoteApi: { listKnownHosts: async () => [], createConnection: async (input) => calls.push(structuredClone(input)) },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  const run = (code) => vm.runInContext(code, context)
  show.value = true
  await nextTick()
  assert.equal(run('form.name'), 'Server copy')
  connection.name = 'S'.repeat(100)
  show.value = false
  await nextTick()
  show.value = true
  await nextTick()
  assert.equal(run('form.name'), 'S'.repeat(100))
  assert.equal(run('form.host'), 'example.com')
  assert.equal(run('password.value'), '')
  await run('save()')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].passwordChange.operation, 'keep')
  assert.equal('password' in calls[0].passwordChange, false)
  assert.equal(show.value, false)
})

test('SSH connection drag starts from the name and reorders below the target', async () => {
  const source = await readFile(new URL('../src/components/remote/ConnectionManager.vue', import.meta.url), 'utf8')
  const script = source.split('<script setup lang="ts">')[1].split('</script>')[0].replace(/^import .*$/gm, '')
  const reordered = []
  const transferred = []
  const dragEvent = {
    preventDefault() {},
    clientY: 30,
    currentTarget: { getBoundingClientRect: () => ({ top: 0, height: 40 }) },
    dataTransfer: {
      effectAllowed: '',
      dropEffect: '',
      setData(_type, value) { transferred.push(value) },
      getData() { return transferred.at(-1) ?? '' },
    },
  }
  const context = vm.createContext({
    ref, onMounted() {}, defineExpose() {}, defineEmits: () => () => {}, t: (key) => key, dragEvent,
    remoteApi: { reorderConnections: async (ids) => { reordered.push([...ids]) } },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  const run = (code) => vm.runInContext(code, context)
  run("connections.value = [{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }, { id: 'c', name: 'C' }]")
  run('startDrag(dragEvent, "a")')
  assert.deepEqual(transferred, ['a'])
  assert.equal(dragEvent.dataTransfer.effectAllowed, 'move')
  run('allowDrop(dragEvent, "b")')
  run('dragOver(dragEvent, "b")')
  assert.equal(dragEvent.dataTransfer.dropEffect, 'move')
  assert.equal(run('dropTarget.value.after'), true)
  await run('drop(dragEvent, "b")')
  assert.deepEqual(reordered, [['b', 'a', 'c']])
  const card = source.slice(source.indexOf('class="remote-connection"'), source.indexOf('remote-connection-name'))
  const name = source.slice(source.indexOf('remote-connection-name'), source.indexOf('remote-connection-endpoint'))
  const actions = source.slice(source.indexOf('remote-connection-actions'), source.indexOf('ConnectionDialog'))
  assert.doesNotMatch(card, /draggable/)
  assert.match(name, /:draggable="!sorting"/)
  assert.match(name, /startDrag/)
  assert.match(card, /@dragenter="allowDrop\(\$event, connection\.id\)"/)
  assert.doesNotMatch(actions, /draggable/)
})

test('remote terminal tabs rename persistently, validate names and preserve edits after failure', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('let saveChain'), source.indexOf('async function persist('))
    + source.slice(source.indexOf('function renameTab('), source.indexOf('function cycleTab('))
  const record = ref({ id: 'workspace', layout: { activeTabId: 'one', tabs: [{ id: 'one', title: 'Terminal' }, { id: 'two', title: 'Other' }] } })
  const rename = ref(null), savingRename = ref(false), dialogOpen = computed(() => !!rename.value)
  const props = { active: true, shortcutsBlocked: false }
  const calls = [], errors = []
  let rejectSave = false
  const context = vm.createContext({
    record, layout: computed(() => record.value.layout), rename, savingRename, dialogOpen, props, busy: ref(false),
    window: { clearTimeout() {} }, ratioTimer: 0,
    asLayout: (value) => value, fail: (error) => errors.push(String(error)),
    remoteApi: { saveProjectLayout: async (id, layout) => {
      calls.push(JSON.parse(JSON.stringify({ id, layout })))
      if (rejectSave) throw new Error('save failed')
    } },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  props.active = false
  context.renameTab('one')
  assert.equal(rename.value, null)
  props.active = true
  props.shortcutsBlocked = true
  context.renameTab('one')
  assert.equal(rename.value, null)
  props.shortcutsBlocked = false
  context.renameTab('one')
  assert.equal(rename.value.title, 'Terminal')
  rename.value.title = '  '
  await context.saveRename()
  assert.equal(calls.length, 0)
  rename.value.title = '  Build server  '
  rejectSave = true
  await context.saveRename()
  assert.equal(record.value.layout.tabs[0].title, 'Terminal')
  assert.equal(rename.value.title, '  Build server  ')
  assert.equal(savingRename.value, false)
  assert.equal(context.busy.value, false)
  assert.match(errors[0], /save failed/)
  rejectSave = false
  const saving = context.saveRename()
  await context.saveRename()
  await saving
  assert.equal(calls.length, 2)
  assert.equal(calls[1].id, 'workspace')
  assert.equal(calls[1].layout.tabs[0].title, 'Build server')
  assert.equal(record.value.layout.tabs[0].title, 'Build server')
  assert.equal(record.value.layout.tabs[1].title, 'Other')
  assert.equal(record.value.layout.activeTabId, 'one')
  assert.equal(rename.value, null)
  context.renameTab('two')
  rename.value.title = 'Cancelled'
  rename.value = null
  await context.saveRename()
  assert.equal(calls.length, 2)
  let prevented = false
  props.shortcutsBlocked = true
  context.renameFromPointer({ preventDefault() { prevented = true } }, 'one')
  assert.equal(prevented, false)
  assert.equal(rename.value, null)
  props.shortcutsBlocked = false
  context.renameFromPointer({ preventDefault() { prevented = true } }, 'one')
  assert.equal(prevented, true)
  assert.equal(rename.value.id, 'one')
  assert.match(source, /@contextmenu="renameFromPointer\(\$event, tab\.id\)"/)
  assert.match(source, /:title="index < 9 \? hint\(`tab\$\{index \+ 1\}`\) : undefined"/)
  assert.match(source, /@mousedown\.middle\.prevent @auxclick\.middle\.prevent\.stop="closeTab\(tab\)"/)
})

test('remote terminal shortcuts cycle tabs, restore focus and respect blocking and overrides', async () => {
  const { matchShortcut } = await loadService('workspace/shortcuts')
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('function cycleTab('), source.indexOf('function askClose('))
  const layout = ref({ activeTabId: 'one', tabs: [{ id: 'one' }, { id: 'two' }, { id: 'three' }] })
  const selected = [], focused = [], added = []
  const props = { active: true, shortcutsBlocked: false, settings: {} }
  const dialogOpen = ref(false)
  const context = vm.createContext({
    layout, props, dialogOpen, matchShortcut, nextTick, busy: ref(false), document: { querySelectorAll: () => [] },
    selectTab(id) { selected.push(id); layout.value.activeTabId = id },
    focusTerminal() { focused.push(layout.value.activeTabId) },
    addTerminal(direction) { added.push(direction ?? true) },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  function press(code, extra = {}) {
    let prevented = false, stopped = false
    context.keydown({
      code, altKey: true, ctrlKey: false, shiftKey: false, metaKey: false, repeat: false,
      preventDefault() { prevented = true }, stopPropagation() { stopped = true }, ...extra,
    })
    assert.equal(prevented, stopped)
    return prevented
  }
  assert.equal(press('ArrowLeft'), true)
  await nextTick()
  assert.equal(layout.value.activeTabId, 'three')
  assert.equal(focused.at(-1), 'three')
  press('ArrowRight')
  await nextTick()
  assert.equal(layout.value.activeTabId, 'one')
  assert.equal(focused.at(-1), 'one')
  press('ArrowRight', { repeat: true })
  assert.equal(selected.length, 2)
  for (const extra of [{ ctrlKey: true }, { shiftKey: true }, { metaKey: true }, { isComposing: true }]) {
    assert.equal(press('ArrowRight', extra), false)
  }
  const newTerminal = { altKey: false, ctrlKey: true, shiftKey: true }
  assert.equal(press('KeyT', newTerminal), true)
  assert.equal(added.length, 1)
  press('KeyT', { ...newTerminal, repeat: true })
  assert.equal(added.length, 1)
  assert.equal(press('KeyT'), false)
  props.active = false
  assert.equal(press('ArrowRight'), false)
  props.active = true
  props.shortcutsBlocked = true
  assert.equal(press('ArrowRight'), false)
  props.shortcutsBlocked = false
  dialogOpen.value = true
  assert.equal(press('ArrowRight'), false)
  dialogOpen.value = false
  props.settings.shortcuts = { nextTab: [{ code: 'KeyN', altKey: true, ctrlKey: false, shiftKey: false, metaKey: false }] }
  assert.equal(press('ArrowRight'), false)
  assert.equal(press('KeyN'), true)
  await nextTick()
  assert.equal(layout.value.activeTabId, 'two')
  layout.value.tabs = [{ id: 'two' }]
  press('KeyN')
  layout.value.tabs = []
  press('KeyN')
  assert.equal(selected.length, 3)
  layout.value.tabs = [{ id: 'one' }, { id: 'two' }]
  layout.value.activeTabId = 'one'
  assert.equal(press('ArrowRight', newTerminal), false)
  assert.equal(press('ArrowLeft', newTerminal), false)
  assert.equal(layout.value.activeTabId, 'one')
  assert.equal(press('Digit2'), true)
  await nextTick()
  assert.equal(layout.value.activeTabId, 'two')
  assert.equal(press('KeyH', newTerminal), true)
  assert.equal(press('KeyJ', newTerminal), true)
  assert.deepEqual(added, [true, 'horizontal', 'vertical'])
  assert.equal(press('ArrowRight', { ...newTerminal, target: { tagName: 'INPUT', closest: () => null } }), false)
})

test('remote projects cycle in sidebar order, restore tab selection and guard dialogs', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteDeck.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('function cycleProject('), source.indexOf('watch(() => [props.active, remote.activeProjectId]'))
    + source.slice(source.indexOf('function modalVisible('), source.indexOf('function cycleProject('))
  const selected = [], focused = []
  const tabs = new Map([['one', 'tab-one'], ['two', 'tab-two']])
  const remote = {
    projects: [{ id: 'one' }, { id: 'two' }, { id: 'empty' }], activeProjectId: 'one', activeTabId: 'tab-one',
    selectProject(id) { selected.push(id); this.activeProjectId = id; this.activeTabId = tabs.get(id) ?? null },
  }
  const props = { active: true, shortcutsBlocked: false }
  const choosing = ref(false), manager = ref(false), closing = ref(null), busy = ref(false)
  let modalShown = false
  const context = vm.createContext({
    props, remote, choosing, manager, closing, busy, nextTick,
    document: { querySelectorAll: () => modalShown ? [{ getClientRects: () => [{}] }] : [] },
    focusTerminal() { if (remote.activeTabId) focused.push(remote.activeTabId) },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  assert.equal(context.cycleProject(1), 'two')
  await nextTick()
  assert.deepEqual(focused, ['tab-two'])
  assert.equal(context.cycleProject(-1), 'one')
  await nextTick()
  assert.equal(focused.at(-1), 'tab-one')
  assert.equal(context.cycleProject(-1), 'empty')
  await nextTick()
  assert.equal(focused.length, 2)
  assert.equal(context.cycleProject(1), 'one')
  await nextTick()
  remote.activeProjectId = 'missing'
  assert.equal(context.cycleProject(-1), 'empty')
  remote.activeProjectId = null
  assert.equal(context.cycleProject(1), 'one')
  await nextTick()
  const count = selected.length
  props.active = false
  assert.equal(context.cycleProject(1), undefined)
  props.active = true
  props.shortcutsBlocked = true
  assert.equal(context.cycleProject(1), undefined)
  props.shortcutsBlocked = false
  modalShown = true
  assert.equal(context.cycleProject(1), undefined)
  modalShown = false
  assert.equal(selected.length, count)
  remote.projects = []
  assert.equal(context.cycleProject(1), undefined)
  remote.projects = [{ id: 'one' }]
  assert.equal(context.cycleProject(1), 'one')
  props.active = false
  const focusCount = focused.length
  await nextTick()
  assert.equal(focused.length, focusCount)
})

test('remote sidebar reveals the requested project without moving keyboard focus', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteSidebar.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('function revealProject('), source.indexOf('defineExpose('))
  const scrolled = []
  const projectList = ref({ querySelectorAll: () => ['one', 'two'].map((id) => ({
    dataset: { projectId: id }, scrollIntoView(options) { scrolled.push([id, options.block, options.inline]) },
  })) })
  const context = vm.createContext({ projectList })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  context.revealProject('two')
  assert.deepEqual(scrolled, [['two', 'nearest', 'nearest']])
  context.revealProject('missing')
  projectList.value = undefined
  context.revealProject('two')
  assert.equal(scrolled.length, 1)
})


const {
  endpointLabel, remotePaneNeedsCloseConfirm, batchCancelId, transferPercent,
  parentPath, joinRemote, partitionSessions, failedRoots, formatBytes, terminalPaneNames,
} = await loadService('remote/workspace')

test('a remote pane uses the tab title only when that tab has one terminal', () => {
  const names = terminalPaneNames(
    [{ id: 'a', name: 'Terminal' }, { id: 'b', name: 'Left' }, { id: 'c', name: 'Right' }],
    [
      { id: 'one', title: 'Build server', root: { type: 'terminal', terminalConfigId: 'a' } },
      { id: 'two', title: 'Split', root: { type: 'split', direction: 'horizontal', ratio: 0.5, children: [
        { type: 'terminal', terminalConfigId: 'b' },
        { type: 'terminal', terminalConfigId: 'c' },
      ] } },
    ],
  )
  assert.equal(names.get('a'), 'Build server')
  assert.equal(names.get('b'), 'Left')
  assert.equal(names.get('c'), 'Right')
})

test('a remote pane mounts a session bound after its first render and follows replacements', async () => {
  const source = ts.transpileModule(await readFile(new URL('../src/services/terminal/registry.ts', import.meta.url), 'utf8'), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText
  const exports = {}
  vm.runInNewContext(source, { exports, require(name) {
    if (name === 'vue') return { markRaw, shallowReactive }
    return {}
  } })
  const registry = new exports.TerminalRegistry(() => 'dark', () => ({}))
  const entry = computed(() => registry.byConfig.get('remote-config'))
  let mounted = null
  let focused = null
  const stop = watch(entry, (value) => { mounted = value?.host ?? null }, { immediate: true, flush: 'post' })
  const status = computed(() => entry.value?.status.state ?? 'not-started')
  const makeEntry = (id) => shallowReactive({
    host: { id }, status: { state: 'starting' },
    client: { attach() {}, focus() { focused = id } },
  })
  try {
    assert.equal(status.value, 'not-started')
    assert.equal(mounted, null)
    await Promise.resolve()
    const first = makeEntry('first')
    registry.bind('remote-config', first, 'ssh-session')
    await nextTick()
    assert.equal(mounted, first.host)
    first.status = { state: 'running' }
    assert.equal(status.value, 'running')
    entry.value.client.focus()
    assert.equal(focused, 'first')

    const replacement = makeEntry('replacement')
    registry.bind('remote-config', replacement, 'new-ssh-session')
    await nextTick()
    assert.equal(mounted, replacement.host)
    registry.byConfig.delete('remote-config')
    await nextTick()
    assert.equal(mounted, null)
    assert.equal(status.value, 'not-started')
  } finally { stop() }
})

test('switching a remote pane replaces the previous terminal host', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteLayoutPane.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('function attach()'), source.indexOf('onMounted(attach)'))
  const first = { id: 'first' }, second = { id: 'second' }
  let resized = 0
  const mount = { children: [], get firstChild() { return this.children[0] }, replaceChildren(...nodes) { this.children = nodes } }
  const entry = ref({ host: first, client: { refreshSize() { resized++ } } })
  const context = { mount: { value: mount }, entry, props: { visible: true } }
  vm.createContext(context)
  vm.runInContext(script, context)
  vm.runInContext('attach()', context)
  assert.deepEqual(mount.children.map((node) => node.id), ['first'])
  entry.value = { host: second, client: { refreshSize() { resized++ } } }
  vm.runInContext('attach()', context)
  vm.runInContext('attach()', context)
  assert.deepEqual(mount.children.map((node) => node.id), ['second'])
  assert.equal(resized, 3)
})

test('selecting a remote terminal tab moves focus to a pane in that tab', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('function selectTab('), source.indexOf('function askClose('))
  const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
  const record = ref({ layout: { activeTabId: 'first', tabs: [
    { id: 'first', root: { type: 'terminal', terminalConfigId: 'pane-first' } },
    { id: 'second', root: { type: 'terminal', terminalConfigId: 'pane-second' } },
  ] } })
  const context = {
    nextTick, connectMissing: async () => {}, record, focused: ref('pane-first'), asLayout: (layout) => layout, leaves: (root) => [root], busy: ref(false),
    persist: async (layout) => { record.value.layout = layout }, fail: (reason) => { throw reason },
  }
  vm.createContext(context)
  vm.runInContext(js, context)
  vm.runInContext("selectTab('second')", context)
  assert.equal(record.value.layout.activeTabId, 'second')
  assert.equal(context.focused.value, 'pane-second')
  context.busy.value = true
  vm.runInContext("selectTab('first')", context)
  assert.equal(record.value.layout.activeTabId, 'second')
  assert.equal(context.focused.value, 'pane-second')
})

test('an older remote layout save cannot overwrite a newer tab rename', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('let saveChain'), source.indexOf('function onUi('))
    + source.slice(source.indexOf('function renameTab('), source.indexOf('function cycleTab('))
  const calls = [], gates = []
  const record = ref({
    id: 'workspace', lastDirectory: '/old',
    layout: { activeTabId: 'one', tabs: [{ id: 'one', title: 'Terminal' }, { id: 'two', title: 'Other' }] },
  })
  const rename = ref(null)
  const context = vm.createContext({
    record, rename, savingRename: ref(false), busy: ref(false),
    layout: computed(() => record.value.layout), dialogOpen: computed(() => !!rename.value),
    props: { active: true, shortcutsBlocked: false }, asLayout: (value) => value,
    window: { clearTimeout() {} }, ratioTimer: 0, fail: (error) => { throw error },
    remoteApi: { saveProjectLayout: async (_id, layout) => {
      calls.push(layout.tabs[0].title)
      await new Promise((resolve) => gates.push(resolve))
    } },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  const first = context.persist({ ...record.value.layout, activeTabId: 'two' })
  await Promise.resolve()
  await Promise.resolve()
  assert.equal(calls.length, 1)
  rename.value = { id: 'one', title: 'Build server' }
  const second = context.saveRename()
  assert.equal(calls.length, 1)
  gates[0]()
  await first
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(calls, ['Terminal', 'Build server'])
  record.value = { ...record.value, lastDirectory: '/kept' }
  gates[1]()
  await second
  assert.equal(record.value.lastDirectory, '/kept')
  assert.equal(record.value.layout.tabs[0].title, 'Build server')
  assert.equal(record.value.layout.activeTabId, 'two')
  assert.equal(rename.value, null)
})

test('middle-click closes one remote terminal tab and ignores a busy control', async () => {
  const deck = await readSource('../src/components/remote/RemoteDeck.vue')
  const workspace = await readSource('../src/components/remote/RemoteWorkspace.vue')
  assert.doesNotMatch(deck, /remote-connection-tabs/)
  assert.match(workspace, /@mousedown\.middle\.prevent @auxclick\.middle\.prevent\.stop="closeTab\(tab\)"/)
  const paneClosing = ref(null), paneBusy = ref(false)
  const paneContext = vm.createContext({
    closing: paneClosing, busy: paneBusy, leaves: (root) => [root],
    t: (_key, params) => params.title, remotePaneNeedsCloseConfirm: () => true,
  })
  vm.runInContext(compileSlice(workspace, 'function askClose(', 'async function confirmClose('), paneContext)
  paneBusy.value = true
  paneContext.closeTab({ id: 'tab', title: 'Build', root: { type: 'terminal', terminalConfigId: 'term' } })
  assert.equal(paneClosing.value, null)
  paneBusy.value = false
  paneContext.closeTab({ id: 'tab', title: 'Build', root: { type: 'terminal', terminalConfigId: 'term' } })
  assert.equal(paneClosing.value.title, 'Build')
  assert.equal(paneClosing.value.tabId, 'tab')
  assert.deepEqual([...paneClosing.value.ids], ['term'])
})

test('remote split ratios wait while a rename or layout save is in progress', async () => {
  const source = await readSource('../src/components/remote/RemoteWorkspace.vue')
  const record = ref({ layout: { activeTabId: 'one', tabs: [{ id: 'one', root: { type: 'split', ratio: 0.5 } }] } })
  const scheduled = []
  const context = vm.createContext({
    record, busy: ref(false), rename: ref(null), ratioTimer: 0, asLayout: (value) => value,
    setRatio: (node, _path, value) => ({ ...node, ratio: value }),
    persist: () => { throw new Error('persist') }, fail() {},
    window: { clearTimeout() {}, setTimeout(callback) { scheduled.push(callback); return 7 } },
  })
  vm.runInContext(compileSlice(source, 'function onRatio(', 'function syncVisibility('), context)
  context.busy.value = true
  context.onRatio([], 0.8)
  assert.equal(record.value.layout.tabs[0].root.ratio, 0.5)
  context.busy.value = false
  context.rename.value = { id: 'one', title: 'Name' }
  context.onRatio([], 0.8)
  assert.equal(scheduled.length, 0)
  context.rename.value = null
  context.onRatio([], 0.8)
  assert.equal(record.value.layout.tabs[0].root.ratio, 0.8)
  assert.equal(scheduled.length, 1)
})

function readSource(path) {
  return readFile(new URL(path, import.meta.url), 'utf8')
}
function compileSlice(source, start, end) {
  return ts.transpileModule(source.slice(source.indexOf(start), source.indexOf(end)), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
}

test('remote projects retain selection and mount lazily without connection tabs', async () => {
  let projects = [{ id: 'p1', name: 'One' }, { id: 'p2', name: 'Two' }]
  const store = await remoteStoreFixture({
    listProjects: async () => projects,
    listConnections: async () => [], listConnectionTabs: async () => [],
  })
  await store.reload()
  assert.equal(store.activeProjectId.value, 'p1')
  assert.equal(store.visitedProjects.size, 0)
  store.selectProject('p2')
  store.visitedProjects.add('p2')
  assert.equal(store.activeProjectId.value, 'p2')
  await store.reload()
  assert.equal(store.activeProjectId.value, 'p2')
  projects = [projects[0]]
  await store.reload()
  assert.equal(store.activeProjectId.value, 'p1')
  assert.equal(store.visitedProjects.has('p2'), false)
})

async function remoteStoreFixture(remoteApi, overrides = {}) {
  const source = await readFile(new URL('../src/stores/remote.ts', import.meta.url), 'utf8')
  const script = source.replace(/^import .*$/gm, '').replace('export const useRemote', 'const useRemote')
  const context = {
    computed, reactive, ref, defineStore: (_id, setup) => setup,
    Channel: class {}, clearRemoteLink() {}, markSftpReady() {}, remoteApi, ...overrides,
  }
  vm.createContext(context)
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
    + '\nconst store = useRemote()', context)
  return vm.runInContext('store', context)
}

test('connection identity changes invalidate only their transport readiness', async (t) => {
  for (const change of [{ host: 'new-host' }, { port: 2222 }, { username: 'other' }, { authType: 'privateKey' }, { privateKeyPath: '/new-key' }]) {
    await t.test(Object.keys(change)[0], async () => {
      let connection = { id: 'server', host: 'old-host', port: 22, username: 'root', authType: 'password', privateKeyPath: null }
      const tabs = [{ id: 'one', projectId: 'p', connectionId: 'server' }, { id: 'two', projectId: 'p', connectionId: 'server' }, { id: 'other', projectId: 'p', connectionId: 'other-server' }]
      const cleared = [], readiness = []
      const store = await remoteStoreFixture({
        listProjects: async () => [{ id: 'p' }], listConnections: async () => [connection], listConnectionTabs: async () => tabs,
      }, { clearRemoteLink: (id) => cleared.push(id), markSftpReady: (...args) => readiness.push(args) })
      await store.reload()
      connection = { ...connection, name: 'renamed' }
      await store.reload()
      connection = { ...connection, ...change }
      await store.reload()
      assert.deepEqual(cleared, ['one', 'two'])
      assert.deepEqual(readiness, [['one', false], ['two', false]])
      assert.equal(store.projects.value.length, 1)
    })
  }
})

test('remote workspaces share one transfer subscription and keep live updates over a stale snapshot', async () => {
  let channel, subscriptions = 0, finishSubscription, finishSnapshot
  const subscribed = new Promise((resolve) => { finishSubscription = resolve })
  const snapshot = new Promise((resolve) => { finishSnapshot = resolve })
  const store = await remoteStoreFixture({
    subscribeTransfers: (events) => { channel = events; subscriptions++; return subscribed },
    listTransfers: () => snapshot,
  })
  const first = store.listenTransfers()
  const second = store.listenTransfers()
  assert.equal(subscriptions, 1)
  finishSubscription(0)
  channel.onmessage({ transfers: [{ id: 'live', workspaceId: 'one' }, { id: 'other', workspaceId: 'two' }] })
  finishSnapshot({ transfers: [{ id: 'stale' }] })
  await Promise.all([first, second])
  await store.listenTransfers()
  assert.equal(subscriptions, 1)
  assert.deepEqual(store.transfers.value.map((item) => item.id), ['live', 'other'])
})

test('a failed transfer subscription can retry without duplicating a successful listener', async () => {
  let subscriptions = 0
  const store = await remoteStoreFixture({
    subscribeTransfers: async () => { if (++subscriptions === 1) throw new Error('offline'); return 0 },
    listTransfers: async () => ({ transfers: [{ id: 'initial' }] }),
  })
  await assert.rejects(store.listenTransfers(), /offline/)
  await store.listenTransfers()
  await store.listenTransfers()
  assert.equal(subscriptions, 2)
  assert.equal(store.transfers.value[0].id, 'initial')
})

test('remote panes always ask before closing, independent of the local confirm setting', () => {
  assert.equal(remotePaneNeedsCloseConfirm(), true)
  assert.equal(endpointLabel('root', 'example.com', 22), 'root@example.com:22')
  assert.equal(batchCancelId('abc'), 'batch:abc')
})

test('scanning and unknown totals are not shown as a percentage', () => {
  assert.equal(transferPercent('scanning', 10, null), null)
  assert.equal(transferPercent('queued', 0, 100), null)
  assert.equal(transferPercent('running', 50, null), null)
  assert.equal(transferPercent('running', 25, 100), 25)
  assert.equal(formatBytes(1536), '1.5 KB')
})

test('a remote session missing from the list is connected again and is not treated as exited', () => {
  const sessions = [{ terminalId: 'live', terminalConfigId: 'keep', status: { state: 'running' } }]
  const result = partitionSessions(['keep', 'gone'], sessions)
  assert.deepEqual(result.attach, [{ configId: 'keep', terminalId: 'live' }])
  assert.deepEqual(result.connect, ['gone'])
  assert.equal(parentPath('/var/log'), '/var')
  assert.equal(parentPath('/'), null)
  assert.equal(joinRemote('/var', 'log'), '/var/log')
  assert.deepEqual(failedRoots([{ relative: 'docs/a.txt', status: 'failed' }, { relative: 'docs/b.txt', status: 'skipped' }]), ['docs'])
})

async function connectionDialogFixture(t, connection = null) {
  const source = await readFile(new URL('../src/components/remote/ConnectionDialog.vue', import.meta.url), 'utf8')
  const script = source.split('<script setup lang="ts">')[1].split('</script>')[0].replace(/^import .*$/gm, '')
  const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
  const calls = [], show = ref(false), scope = effectScope()
  const context = {
    computed, reactive, ref, watch, defineProps: () => ({ connection }),
    defineModel: () => show, defineEmits: () => () => {}, t: (key) => key,
    remoteApi: {
      listKnownHosts: async () => [],
      createConnection: async (input) => { calls.push(JSON.parse(JSON.stringify(input))); if (context.fail) throw new Error('store failed') },
      updateConnection: async (_id, input) => { calls.push(JSON.parse(JSON.stringify(input))) },
    },
  }
  vm.createContext(context)
  scope.run(() => vm.runInContext(js, context))
  t.after(() => scope.stop())
  show.value = true
  await nextTick()
  return { calls, show, context, run: (code) => vm.runInContext(code, context) }
}

test('connection dialog preserves password whitespace and clears it after success or failure', async (t) => {
  const f = await connectionDialogFixture(t)
  f.run("Object.assign(form, {name: 'lab', host: 'example.com', username: 'root'}); password.value = '  preset  '")
  await f.run('save()')
  assert.deepEqual(f.calls[0].passwordChange, { operation: 'set', password: '  preset  ' })
  assert.equal(f.run('password.value'), '')
  assert.equal(f.show.value, false)
  f.show.value = true
  await nextTick()
  f.context.fail = true
  f.run("password.value = 'retry'")
  await f.run('save()')
  assert.equal(f.show.value, true)
  assert.equal(f.run('password.value'), '')
  assert.match(f.run('error.value'), /store failed/)
})

test('editing a connection keeps, replaces or explicitly clears the saved password', async (t) => {
  const f = await connectionDialogFixture(t, { id: 'connection', name: 'lab', host: 'example.com', port: 22, username: 'root', authType: 'password', privateKeyPath: null, hasSavedPassword: true })
  assert.equal(f.run('password.value'), '')
  await f.run('save()')
  assert.deepEqual(f.calls[0].passwordChange, { operation: 'keep' })
  f.show.value = true
  await nextTick()
  f.run("password.value = 'replacement'")
  await f.run('save()')
  assert.deepEqual(f.calls[1].passwordChange, { operation: 'set', password: 'replacement' })
  f.show.value = true
  await nextTick()
  f.run('clearPassword.value = true')
  await f.run('save()')
  assert.deepEqual(f.calls[2].passwordChange, { operation: 'clear' })
})

test('login remembers a password only when selected and permits retry after a store failure', async () => {
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('async function submitSecret()'), source.indexOf('async function answer('))
  const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
  const calls = [], errors = [], credential = ref({ batchId: 'one', authType: 'password' })
  const context = {
    credential, secret: ref('temporary'), savePassword: ref(false), submittingSecret: ref(false),
    fail: (error) => errors.push(String(error)), batchCancelId, nextPrompts() {},
    remoteApi: {
      submitCredential: async (...args) => { calls.push(args); if (context.failStore) throw new Error('store failed') },
      cancelAttempt: async () => {},
    },
  }
  vm.createContext(context)
  vm.runInContext(js, context)
  await vm.runInContext('submitSecret()', context)
  assert.deepEqual(calls[0], ['one', 'temporary', false])
  assert.equal(credential.value, null)
  credential.value = { batchId: 'two', authType: 'password' }
  context.secret.value = 'saved'
  context.savePassword.value = true
  context.failStore = true
  await vm.runInContext('submitSecret()', context)
  assert.equal(credential.value.batchId, 'two')
  assert.equal(context.secret.value, '')
  assert.equal(context.submittingSecret.value, false)
  assert.match(errors[0], /store failed/)
  context.failStore = false
  context.secret.value = 'saved'
  await vm.runInContext('submitSecret()', context)
  assert.deepEqual(calls[2], ['two', 'saved', true])
  assert.equal(context.savePassword.value, false)
  credential.value = { batchId: 'three', authType: 'privateKey' }
  context.secret.value = 'passphrase'
  context.savePassword.value = true
  await vm.runInContext('submitSecret()', context)
  assert.deepEqual(calls[3], ['three', 'passphrase', false])
})

test('SFTP status events work before and after the connection command returns', async (t) => {
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('function nextPrompts('), source.indexOf('async function connectMissing('))
  const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
  for (const early of [true, false]) {
    for (const state of ['ready', 'failed']) {
      await t.test(`${state} ${early ? 'before' : 'after'} command response`, async () => {
        const ready = [], browsed = [], errors = []
        let events
        const event = { type: 'attempt', attemptId: 'sftp-new', state, message: state === 'failed' ? 'Authentication failed' : undefined }
        const context = {
          ref, launching: ref(false), sftpAttempts: new Map(), credentialQueue: [], challengeQueue: [],
          remote: { connections: [{ id: 'connection', authType: 'password' }] },
          fileContext: ref({ id: 'workspace', connectionId: 'connection', lastDirectory: '/home' }),
          record: ref({ contexts: [{ id: 'workspace', connectionId: 'connection', lastDirectory: '/home' }] }), pathInput: ref('.'), directory: ref('.'), terminalDirectory: ref(null),
          sftpUp: ref(false), sftpError: ref(''), filesVisible: ref(true), savePassword: ref(false), credential: ref(null), challenge: ref(null),
          Channel: class {}, setRemoteLink() {}, syncVisibility() {}, t: (key) => key, batchCancelId,
          markSftpReady: (...args) => ready.push(args), browse: (path) => browsed.push(path), fail: (error) => errors.push(error),
          remoteApi: {
            beginConnect: async (_id, channel) => { events = channel; return { batchId: 'batch' } },
            connectSftp: async () => { if (early) events.onmessage(event); return { attemptId: 'sftp-new' } },
            sealConnect: async () => {},
          },
        }
        vm.createContext(context)
        vm.runInContext(js, context)
        await vm.runInContext('launch([], true)', context)
        if (!early) events.onmessage(event)
        assert.deepEqual(errors, [])
        assert.deepEqual(ready, [['workspace', state === 'ready']])
        assert.equal(context.sftpUp.value, state === 'ready')
        assert.deepEqual(browsed, state === 'ready' ? ['/home'] : [])
        assert.equal(context.sftpError.value, state === 'failed' ? 'Authentication failed' : '')
      })
    }
  }
})


test('new tabs and splits choose a connection before creating and serialize against rename', async (t) => {
  const source = await readSource('../src/components/remote/RemoteWorkspace.vue')
  const { splitLeaf } = await loadService('workspace/layout')
  const script = compileSlice(source, 'let saveChain', 'function nextPrompts(')
    + compileSlice(source, 'function renameTab(', 'function cycleTab(')
    + compileSlice(source, 'async function addTerminal(', 'async function activateFiles(')
  for (const direction of [undefined, 'horizontal', 'vertical']) {
    await t.test(direction ?? 'tab', async () => {
      const original = {
        id: 'project', contexts: [{ id: 'a-context', connectionId: 'a' }],
        terminals: [{ id: 'a-terminal', remoteWorkspaceId: 'a-context', connectionId: 'a', name: 'A' }],
        layout: { schemaVersion: 1, activeTabId: 'first', tabs: [{ id: 'first', title: 'Existing', root: { type: 'terminal', terminalConfigId: 'a-terminal' } }] },
      }
      const record = ref(structuredClone(original)), focused = ref('a-terminal')
      const choosing = ref(false), rename = ref(null), busy = ref(false)
      const calls = [], saves = [], launches = [], removed = [], errors = []
      let finishSave, holdSave = false, failSave = false
      const context = vm.createContext({
        record, focused, choosing, rename, busy, creating: ref(false), chosen: ref(null), creationTarget: null,
        savingRename: ref(false), launching: ref(false), props: { active: true, shortcutsBlocked: false },
        dialogOpen: computed(() => choosing.value || !!rename.value), layout: computed(() => record.value.layout),
        fileContext: computed(() => record.value.contexts.find((item) => item.connectionId === record.value.terminals.find((item) => item.id === focused.value)?.connectionId)),
        remote: { connections: [{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }], reload: async () => {} },
        asLayout: (value) => value, splitLeaf, crypto: { randomUUID: () => 'new-tab' },
        window: { clearTimeout() {} }, ratioTimer: 0, isSftpReady: () => true,
        launch: async (...args) => launches.push(args), fail: (reason) => errors.push(String(reason)),
        remoteApi: {
          createProjectTerminal: async (...args) => { calls.push(args); return { id: 'b-terminal', remoteWorkspaceId: 'b-context', connectionId: 'b', name: 'B' } },
          getProjectWorkspace: async () => ({ ...original, contexts: [...original.contexts, { id: 'b-context', connectionId: 'b' }], terminals: [...original.terminals, { id: 'b-terminal', remoteWorkspaceId: 'b-context', connectionId: 'b', name: 'B' }] }),
          removeTerminal: async (...args) => removed.push(args),
          saveProjectLayout: async (id, layout) => { saves.push(JSON.parse(JSON.stringify({ id, layout }))); if (failSave) throw new Error('save failed'); if (holdSave) await new Promise((resolve) => { finishSave = resolve }) },
        },
      })
      vm.runInContext(script, context)
      await context.addTerminal(direction)
      assert.equal(choosing.value, true)
      assert.equal(context.chosen.value, null)
      assert.equal(calls.length, 0)
      choosing.value = false
      assert.deepEqual(JSON.parse(JSON.stringify(record.value)), original)
      await context.addTerminal(direction)
      context.chosen.value = 'b'
      await context.createChosenTerminal()
      assert.deepEqual(calls, [['project', 'b', 'B']])
      assert.equal(record.value.terminals.find((item) => item.id === 'b-terminal').connectionId, 'b')
      assert.equal(record.value.layout.tabs.length, direction ? 1 : 2)
      if (direction) {
        assert.equal(record.value.layout.tabs[0].root.direction, direction)
        assert.equal(record.value.layout.tabs[0].root.children[0].terminalConfigId, 'a-terminal')
      }
      assert.equal(focused.value, 'b-terminal')
      assert.equal(choosing.value, false)
      assert.deepEqual(JSON.parse(JSON.stringify(launches)), [[['b-terminal'], false]])
      context.renameTab('first')
      rename.value.title = 'Renamed'
      holdSave = true
      const saving = context.saveRename()
      await new Promise((resolve) => setImmediate(resolve))
      await context.addTerminal(direction)
      assert.equal(choosing.value, false)
      finishSave()
      await saving
      assert.equal(record.value.layout.tabs[0].title, 'Renamed')
      holdSave = false
      await context.addTerminal()
      context.chosen.value = 'b'
      failSave = true
      await context.createChosenTerminal()
      assert.equal(choosing.value, true)
      assert.equal(record.value.layout.tabs[0].title, 'Renamed')
      assert.deepEqual(removed, [['b-context', 'b-terminal']])
      assert.match(errors[0], /save failed/)
    })
  }
})

test('authentication prompts from different connections queue without replacing one another', async () => {
  const source = await readSource('../src/components/remote/RemoteWorkspace.vue')
  const context = vm.createContext({
    credential: ref(null), challenge: ref(null), launching: ref(false), credentialQueue: [], challengeQueue: [],
    record: ref({ contexts: [{ id: 'a' }, { id: 'b' }] }),
    remote: { connections: [{ id: 'a', authType: 'password' }, { id: 'b', authType: 'privateKey' }] },
  })
  vm.runInContext(compileSlice(source, 'function nextPrompts(', 'async function launchContext('), context)
  context.onUi({ type: 'credentialRequired', batchId: 'batch-a' }, { id: 'a', connectionId: 'a' })
  context.onUi({ type: 'credentialRequired', batchId: 'batch-b' }, { id: 'b', connectionId: 'b' })
  assert.equal(context.credential.value.connectionId, 'a')
  context.credential.value = null
  context.nextPrompts()
  assert.equal(context.credential.value.connectionId, 'b')
  assert.equal(context.credential.value.authType, 'privateKey')
  context.onUi({ type: 'challenge', challengeId: 'host-a' }, { id: 'a', connectionId: 'a' })
  context.onUi({ type: 'challenge', challengeId: 'host-b' }, { id: 'b', connectionId: 'b' })
  assert.equal(context.challenge.value, null)
  context.credential.value = null
  context.nextPrompts()
  assert.equal(context.challenge.value.challengeId, 'host-a')
  context.challenge.value = null
  context.nextPrompts()
  assert.equal(context.challenge.value.challengeId, 'host-b')
})


test('mixed layouts batch authentication per profile while keeping each terminal on its selected connection', async () => {
  const source = await readSource('../src/components/remote/RemoteWorkspace.vue')
  const contexts = [{ id: 'one', connectionId: 'a' }, { id: 'two', connectionId: 'b' }, { id: 'legacy-a', connectionId: 'a' }]
  const calls = []
  const context = vm.createContext({
    record: ref({ contexts, terminals: [
      { id: 'a1', connectionId: 'a', remoteWorkspaceId: 'one' },
      { id: 'a2', connectionId: 'a', remoteWorkspaceId: 'legacy-a' },
      { id: 'b1', connectionId: 'b', remoteWorkspaceId: 'two' },
    ] }),
    fileContext: ref(contexts[0]), launching: ref(false),
    launchContext: async (ids, target, sftp) => calls.push([Array.from(ids), target.connectionId, sftp]),
    nextPrompts() {}, syncVisibility() {}, activateFiles() {},
  })
  vm.runInContext(compileSlice(source, 'async function launch(', 'async function connectMissing('), context)
  await context.launch(['a1', 'a2', 'b1'], true)
  assert.deepEqual(calls, [[['a1', 'a2'], 'a', true], [['b1'], 'b', false]])
  assert.equal(context.launching.value, false)
})
