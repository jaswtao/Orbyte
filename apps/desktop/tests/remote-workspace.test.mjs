/** Remote workspace rules that do not need a server. Author: jaswtao · Date: 2026-10-31 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { computed, nextTick, watch, markRaw, shallowReactive, reactive, ref, effectScope } from 'vue'

const {
  endpointLabel, remotePaneNeedsCloseConfirm, batchCancelId, transferPercent,
  parentPath, joinRemote, partitionSessions, failedRoots, formatBytes,
} = await loadService('remote/workspace')

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
    record, focused: ref('pane-first'), asLayout: (layout) => layout, leaves: (root) => [root],
    persist: async (layout) => { record.value.layout = layout }, fail: (reason) => { throw reason },
  }
  vm.createContext(context)
  vm.runInContext(js, context)
  vm.runInContext("selectTab('second')", context)
  assert.equal(record.value.layout.activeTabId, 'second')
  assert.equal(context.focused.value, 'pane-second')
})

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
