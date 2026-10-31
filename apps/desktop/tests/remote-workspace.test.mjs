/** Remote workspace rules that do not need a server. Author: jaswtao · Date: 2026-10-31 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { computed, nextTick, watch, markRaw, shallowReactive, reactive, ref, effectScope } from 'vue'

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
    remoteApi: { saveWorkspace: async (id, layout) => {
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

test('remote tab rename and terminal creation cannot overwrite each other while requests are pending', async (t) => {
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const { splitLeaf } = await loadService('workspace/layout')
  const script = source.slice(source.indexOf('function renameTab('), source.indexOf('function cycleTab('))
    + source.slice(source.indexOf('let saveChain'), source.indexOf('function onUi('))
    + source.slice(source.indexOf('async function addTerminal('), source.indexOf('function onRatio('))
  for (const direction of [undefined, 'horizontal', 'vertical']) {
    await t.test(direction ?? 'new tab', async () => {
      const record = ref({
        id: 'workspace', terminals: [{ id: 'terminal-one' }],
        layout: { activeTabId: 'one', tabs: [{ id: 'one', title: 'Terminal', root: { type: 'terminal', terminalConfigId: 'terminal-one' } }] },
      })
      const rename = ref(null), savingRename = ref(false), busy = ref(false)
      const calls = [], errors = [], clearedTimers = []
      let finishCreate, finishSave, holdSave = false, failCreate = false, creates = 0, connectedWhileBusy = null
      const context = vm.createContext({
        record, rename, savingRename, busy, layout: computed(() => record.value.layout),
        focused: ref('terminal-one'), dialogOpen: computed(() => !!rename.value),
        props: { active: true, shortcutsBlocked: false }, asLayout: (value) => value,
        window: { clearTimeout: (timer) => clearedTimers.push(timer) }, ratioTimer: 42,
        crypto: { randomUUID: () => 'two' }, t: () => 'Terminal', splitLeaf,
        launch: async () => { connectedWhileBusy = busy.value }, isSftpReady: () => true, fail: (reason) => errors.push(String(reason)),
        remoteApi: {
          createTerminal: () => {
            creates += 1
            if (failCreate) return Promise.reject(new Error('create failed'))
            return new Promise((resolve) => { finishCreate = resolve })
          },
          saveWorkspace: async (id, layout) => {
            calls.push(JSON.parse(JSON.stringify({ id, layout })))
            if (holdSave) await new Promise((resolve) => { finishSave = resolve })
          },
        },
      })
      vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
      const adding = context.addTerminal(direction)
      assert.equal(busy.value, true)
      context.renameTab('one')
      assert.equal(rename.value, null)
      finishCreate('terminal-two')
      await adding
      assert.equal(busy.value, false)
      assert.equal(connectedWhileBusy, false)
      assert.equal(record.value.terminals.length, 2)
      assert.equal(record.value.layout.tabs.length, direction ? 1 : 2)
      if (direction) assert.equal(record.value.layout.tabs[0].root.type, 'split')
      context.renameTab('one')
      rename.value.title = 'Renamed'
      holdSave = true
      const saving = context.saveRename()
      assert.equal(busy.value, true)
      await context.addTerminal(direction)
      assert.equal(creates, 1)
      assert.deepEqual(clearedTimers, [42, 42])
      const expected = JSON.parse(JSON.stringify(record.value))
      for (const tab of expected.layout.tabs) if (tab.id === 'one') tab.title = 'Renamed'
      assert.equal(calls.length, 2)
      assert.deepEqual(calls[1], { id: 'workspace', layout: expected.layout })
      record.value = { ...record.value, lastDirectory: '/kept' }
      finishSave()
      await saving
      assert.equal(record.value.lastDirectory, '/kept')
      assert.equal(record.value.layout.tabs.find((tab) => tab.id === 'one').title, 'Renamed')
      assert.equal(record.value.terminals.length, 2)
      assert.equal(rename.value, null)
      assert.equal(savingRename.value, false)
      assert.equal(busy.value, false)
      failCreate = true
      await context.addTerminal(direction)
      assert.match(errors[0], /create failed/)
      assert.equal(busy.value, false)
      context.renameTab('one')
      assert.equal(rename.value.title, 'Renamed')
    })
  }
})

test('remote terminal shortcuts cycle tabs, restore focus and respect blocking and overrides', async () => {
  const { matchShortcut } = await loadService('workspace/shortcuts')
  const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('function cycleTab('), source.indexOf('function askClose('))
  const layout = ref({ activeTabId: 'one', tabs: [{ id: 'one' }, { id: 'two' }, { id: 'three' }] })
  const selected = [], focused = []
  const props = { active: true, shortcutsBlocked: false, settings: {} }
  const dialogOpen = ref(false)
  const context = vm.createContext({
    layout, props, dialogOpen, matchShortcut, nextTick,
    selectTab(id) { selected.push(id); layout.value.activeTabId = id },
    focusTerminal() { focused.push(layout.value.activeTabId) },
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
    record, focused: ref('pane-first'), asLayout: (layout) => layout, leaves: (root) => [root], busy: ref(false),
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
    remoteApi: { saveWorkspace: async (_id, layout) => {
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

test('middle-click closes one remote connection or terminal tab and ignores a busy control', async () => {
  const deck = await readSource('../src/components/remote/RemoteDeck.vue')
  const workspace = await readSource('../src/components/remote/RemoteWorkspace.vue')
  assert.match(deck, /@mousedown\.middle\.prevent @auxclick\.middle\.prevent\.stop="middleClose\(tab\.id\)"/)
  const closing = ref(null), busy = ref(false)
  const deckContext = vm.createContext({ closing, busy })
  vm.runInContext(compileSlice(deck, 'function middleClose(', 'defineExpose'), deckContext)
  deckContext.middleClose('tab-a')
  assert.equal(closing.value, 'tab-a')
  busy.value = true
  deckContext.middleClose('tab-b')
  assert.equal(closing.value, 'tab-a')
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

test('remote projects retain independent tabs without automatically connecting restored tabs', async () => {
  const source = await readFile(new URL('../src/stores/remote.ts', import.meta.url), 'utf8')
  const script = source.replace(/^import .*$/gm, '').replace('export const useRemote', 'const useRemote')
  const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
  let tabs = [
    { id: 'one', projectId: 'p1', connectionId: 'server' },
    { id: 'two', projectId: 'p2', connectionId: 'server' },
  ]
  const context = {
    computed, reactive, ref, defineStore: (_id, setup) => setup, clearRemoteLink() {}, markSftpReady() {},
    remoteApi: {
      listProjects: async () => [{ id: 'p1', name: 'One' }, { id: 'p2', name: 'Two' }],
      listConnections: async () => [{ id: 'server' }],
      listConnectionTabs: async () => [...tabs],
      openConnectionTab: async (projectId, connectionId) => {
        const tab = { id: 'new', projectId, connectionId }
        tabs.push(tab)
        return tab
      },
      closeConnectionTab: async (id) => { tabs = tabs.filter((tab) => tab.id !== id) },
    },
  }
  vm.createContext(context)
  vm.runInContext(js + '\nconst store = useRemote()', context)
  const store = vm.runInContext('store', context)
  await store.reload()
  assert.equal(store.visited.size, 0)
  store.selectProject('p2')
  assert.equal(store.activeTabId.value, 'two')
  assert.deepEqual([...store.visited], ['two'])
  store.selectProject('p1')
  await store.openConnection('server')
  assert.deepEqual(store.projectTabs.value.map((tab) => tab.id), ['one', 'new'])
  assert.equal(store.activeTabId.value, 'new')
  store.selectProject('p2')
  assert.equal(store.activeTabId.value, 'two')
  await store.closeTab('new')
  assert.equal(store.visited.has('new'), false)
  assert.equal(store.activeTabId.value, 'two')
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

test('connection identity changes discard mounted views and readiness while preserving tabs', async (t) => {
  for (const change of [{ host: 'new-host' }, { port: 2222 }, { username: 'other' }, { authType: 'privateKey' }, { privateKeyPath: '/new-key' }]) {
    await t.test(Object.keys(change)[0], async () => {
      let connection = { id: 'server', host: 'old-host', port: 22, username: 'root', authType: 'password', privateKeyPath: null }
      const tabs = [{ id: 'one', projectId: 'p', connectionId: 'server' }, { id: 'two', projectId: 'p', connectionId: 'server' }, { id: 'other', projectId: 'p', connectionId: 'other-server' }]
      const cleared = [], readiness = []
      const store = await remoteStoreFixture({
        listProjects: async () => [{ id: 'p' }], listConnections: async () => [connection], listConnectionTabs: async () => tabs,
      }, { clearRemoteLink: (id) => cleared.push(id), markSftpReady: (...args) => readiness.push(args) })
      await store.reload()
      for (const tab of tabs) store.selectTab(tab.id)
      connection = { ...connection, name: 'renamed' }
      await store.reload()
      assert.equal(store.visited.size, 3)
      connection = { ...connection, ...change }
      await store.reload()
      assert.deepEqual([...store.visited], ['other'])
      assert.deepEqual(cleared, ['one', 'two'])
      assert.deepEqual(readiness, [['one', false], ['two', false]])
      assert.equal(store.tabs.value.length, 3)
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
    fail: (error) => errors.push(String(error)), batchCancelId,
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
  const script = source.slice(source.indexOf('const launching ='), source.indexOf('let browseId ='))
    + source.slice(source.indexOf('function onUi('), source.indexOf('async function connectMissing('))
  const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
  for (const early of [true, false]) {
    for (const state of ['ready', 'failed']) {
      await t.test(`${state} ${early ? 'before' : 'after'} command response`, async () => {
        const ready = [], browsed = [], errors = []
        let events
        const event = { type: 'attempt', attemptId: 'sftp-new', state, message: state === 'failed' ? 'Authentication failed' : undefined }
        const context = {
          ref, props: { connection: { id: 'connection', authType: 'password' } },
          record: ref({ id: 'workspace', lastDirectory: '/home' }), pathInput: ref('.'),
          sftpUp: ref(false), sftpError: ref(''), savePassword: ref(false), credential: ref(null), challenge: ref(null),
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
