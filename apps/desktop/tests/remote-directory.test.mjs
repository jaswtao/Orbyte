/** Remote directory reporting and focused-pane synchronization. Author: jaswtao · Date: 2026-10-31 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import xterm from '@xterm/xterm'
import { computed, effectScope, nextTick, reactive, ref, shallowReactive, watch } from 'vue'
import { loadService } from './load-service.mjs'

const { remoteDirectory } = await loadService('remote/directory')
const { joinRemote, parentPath } = await loadService('remote/workspace')
const source = await readFile(new URL('../src/components/remote/RemoteWorkspace.vue', import.meta.url), 'utf8')
const flush = () => new Promise((resolve) => setImmediate(resolve))

test('remote OSC paths preserve POSIX paths and ignore URI host names', () => {
  assert.equal(remoteDirectory(9, '9;/home/中文 folder'), '/home/中文 folder')
  assert.equal(remoteDirectory(9, '9;/'), '/')
  assert.equal(remoteDirectory(7, 'file://remote.example/home/%E4%B8%AD%E6%96%87%20folder'), '/home/中文 folder')
  assert.equal(remoteDirectory(7, 'file://remote.example/C:/folder'), '/C:/folder')
  for (const [code, data] of [[9, '9;relative'], [9, '9;C:\\folder'], [9, '9;/bad\npath'],
    [7, 'file:///bad%0apath'], [7, 'file:///bad%xx'], [7, 'https://example.com/path'],
    [133, 'A'], [9, `9;/${'x'.repeat(8192)}`]]) assert.equal(remoteDirectory(code, data), null)
})

test('fragmented OSC reports produce only complete remote paths', async (t) => {
  const term = new xterm.Terminal({ cols: 80, rows: 5 })
  t.after(() => term.dispose())
  const paths = []
  for (const code of [9, 7]) term.parser.registerOscHandler(code, (data) => {
    const path = remoteDirectory(code, data)
    if (path !== null) paths.push(path)
    return true
  })
  const write = (data) => new Promise((resolve) => term.write(data, resolve))
  await write('\x1b]9;9;/home/中')
  assert.deepEqual(paths, [])
  await write('文 folder\x07\x1b]7;file://remote/var/log')
  assert.deepEqual(paths, ['/home/中文 folder'])
  await write('\x1b\\')
  assert.deepEqual(paths, ['/home/中文 folder', '/var/log'])
})

test('binding a new remote session clears its directory while resubscribing preserves it', async () => {
  const source = await readFile(new URL('../src/services/terminal/registry.ts', import.meta.url), 'utf8')
  const script = source.slice(source.indexOf('  bind('), source.indexOf('  remove('))
  const code = ts.transpileModule(`class Registry { byConfig = new Map(); ${script} }`, {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText
  const context = vm.createContext({})
  vm.runInContext(`${code}; globalThis.registry = new Registry()`, context)
  const entry = { remote: true, shell: { cwd: '/old' }, client: { id: 'old', attach(id) { this.id = id } } }
  context.registry.bind('terminal', entry, 'new')
  assert.equal(entry.shell.cwd, null)
  entry.shell = { cwd: '/new' }
  context.registry.bind('terminal', entry, 'new')
  assert.equal(entry.shell.cwd, '/new')
})

function fixture(t) {
  const requests = [], cancelled = [], saved = [], errors = []
  const registry = { byConfig: shallowReactive(new Map([
    ['one', shallowReactive({ shell: { cwd: '/home/one' } })],
    ['two', shallowReactive({ shell: { cwd: '/home/two' } })],
  ])) }
  const focused = ref('one'), props = reactive({ active: true })
  const context = vm.createContext({
    ref, watch, props, focused, registry,
    terminalDirectory: computed(() => registry.byConfig.get(focused.value)?.shell.cwd ?? null),
    record: ref({ id: 'workspace', lastDirectory: '/home' }),
    pathInput: ref('/home'), directory: ref('/home'), browsing: ref(false), sftpUp: ref(true),
    selected: ref([]), entries: ref([]), sftpError: ref(''), browseId: '', browseGeneration: 0,
    Channel: class {}, isSftpReady: () => context.sftpUp.value,
    fail: (error) => { const message = String(error); errors.push(message); return message },
    remoteApi: {
      listDirectory: async (workspace, path, channel) => {
        const id = `browse-${requests.length}`
        requests.push({ workspace, path, channel, id })
        if (context.holdRequest) await context.holdRequest
        return id
      },
      cancelBrowse: async (id) => cancelled.push(id),
      setDirectory: async (...args) => { saved.push(args); if (context.holdSave) await context.holdSave },
    },
  })
  const script = source.slice(source.indexOf('let directorySaveChain'), source.indexOf('function openEntry('))
  const scope = effectScope()
  scope.run(() => vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context))
  t.after(() => scope.stop())
  const run = (code) => vm.runInContext(code, context)
  const batch = (index, options = {}) => requests[index].channel.onmessage({
    path: requests[index].path, entries: [{ name: `file-${index}`, kind: 'file' }], done: true, message: null, ...options,
  })
  return { context, run, requests, cancelled, saved, errors, batch, registry, focused, props }
}

test('only the focused terminal follows directories, including pane and workspace switches', async (t) => {
  const f = fixture(t)
  f.run('followTerminalDirectory()')
  await flush()
  assert.equal(f.requests[0].path, '/home/one')
  f.batch(0)
  f.registry.byConfig.get('two').shell = { cwd: '/var/log' }
  await nextTick()
  assert.equal(f.requests.length, 1)
  f.focused.value = 'two'
  await flush()
  assert.equal(f.requests[1].path, '/var/log')
  f.batch(1)
  f.registry.byConfig.get('two').shell = { cwd: '/var/log' }
  await flush()
  assert.equal(f.requests.length, 2)
  f.props.active = false
  f.registry.byConfig.get('two').shell = { cwd: '/tmp' }
  await flush()
  assert.equal(f.requests.length, 2)
  f.props.active = true
  await flush()
  assert.equal(f.requests[2].path, '/tmp')
})

test('SFTP readiness uses the latest reported directory and reconnect clears the old path', async (t) => {
  const f = fixture(t)
  f.context.sftpUp.value = false
  f.registry.byConfig.get('one').shell = { cwd: '/latest' }
  await nextTick()
  assert.equal(f.requests.length, 0)
  // This is the same ready-event handler used by the SSH connection channel.
  const script = source.slice(source.indexOf('function onUi('), source.indexOf('async function launch('))
  Object.assign(f.context, { setRemoteLink() {}, sftpAttemptId: 'sftp', markSftpReady() {}, t: (key) => key })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, f.context)
  f.run("onUi({ type: 'attempt', attemptId: 'sftp', state: 'ready' })")
  await flush()
  assert.equal(f.requests[0].path, '/latest')
  f.registry.byConfig.get('one').shell = { cwd: null }
  await nextTick()
  assert.equal(f.context.terminalDirectory.value, null)
  assert.equal(f.requests.length, 1)
})

test('rapid changes cancel pending listings and ignore stale batches and command responses', async (t) => {
  const f = fixture(t)
  let release
  f.context.holdRequest = new Promise((resolve) => { release = resolve })
  const old = f.run("browse('/old')")
  f.context.holdRequest = null
  await f.run("browse('/new')")
  f.batch(1, { path: '/canonical/new' })
  f.batch(0)
  release()
  await old
  await flush()
  assert.equal(f.context.directory.value, '/canonical/new')
  assert.equal(f.context.pathInput.value, '/canonical/new')
  assert.equal(f.context.entries.value[0].name, 'file-1')
  assert.deepEqual(f.cancelled, ['browse-0'])
  assert.deepEqual(f.saved, [['workspace', '/canonical/new']])
  assert.equal(f.context.record.value.lastDirectory, '/canonical/new')
})

test('failed listings clear partial entries and retain the terminal path and error', async (t) => {
  const f = fixture(t)
  await f.run("browse('/denied')")
  f.batch(0, { done: false })
  assert.equal(f.context.entries.value.length, 1)
  f.batch(0, { entries: [], message: 'Permission denied' })
  assert.equal(f.context.entries.value.length, 0)
  assert.equal(f.context.directory.value, '/denied')
  assert.equal(f.context.sftpError.value, 'Permission denied')
  assert.equal(f.context.browsing.value, false)
  assert.deepEqual(f.saved, [])
})

test('submitted relative paths resolve to absolute directories with usable parent navigation', async (t) => {
  const f = fixture(t)
  const resolved = []
  f.context.remoteApi.canonicalize = async (...args) => { resolved.push(args); return '/home/projects' }
  const script = source.slice(source.indexOf('async function submitPath('), source.indexOf('async function makeDirectory('))
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, f.context)
  f.context.pathInput.value = 'projects'
  await f.run('submitPath()')
  assert.deepEqual(resolved, [['workspace', 'projects']])
  assert.equal(f.requests[0].path, '/home/projects')
  f.batch(0)
  await flush()
  assert.equal(f.context.directory.value, '/home/projects')
  assert.equal(f.context.pathInput.value, '/home/projects')
  assert.equal(parentPath(f.context.directory.value), '/home')
  assert.deepEqual(f.saved, [['workspace', '/home/projects']])
})

test('late path resolution cannot replace a newer terminal directory', async (t) => {
  const f = fixture(t)
  let resolve
  f.context.remoteApi.canonicalize = () => new Promise((done) => { resolve = done })
  const pending = f.run("browse('projects', true)")
  await f.run("browse('/new')")
  f.batch(0)
  resolve('/home/projects')
  await pending
  assert.equal(f.requests.length, 1)
  assert.equal(f.context.directory.value, '/new')
  assert.equal(f.context.browsing.value, false)
})

test('failed path resolution stops browsing without listing or saving an unresolved path', async (t) => {
  const f = fixture(t)
  f.context.remoteApi.canonicalize = async () => { throw new Error('Cannot resolve directory') }
  await f.run("browse('missing', true)")
  assert.equal(f.context.browsing.value, false)
  assert.equal(f.requests.length, 0)
  assert.deepEqual(f.saved, [])
  assert.match(f.context.sftpError.value, /Cannot resolve directory/)
})

test('directory persistence is serialized so an older save cannot win', async (t) => {
  const f = fixture(t)
  let release
  f.context.holdSave = new Promise((resolve) => { release = resolve })
  await f.run("browse('/old')")
  f.batch(0)
  await flush()
  await f.run("browse('/new')")
  f.batch(1)
  await flush()
  assert.deepEqual(f.saved, [['workspace', '/old']])
  f.context.holdSave = null
  release()
  await flush()
  assert.deepEqual(f.saved, [['workspace', '/old'], ['workspace', '/new']])
})

test('manual browsing survives unchanged prompts and the next directory change resumes following', async (t) => {
  const f = fixture(t)
  await f.run("browse('/manual')")
  f.batch(0)
  f.registry.byConfig.get('one').shell = { cwd: '/home/one' }
  await flush()
  assert.equal(f.requests.length, 1)
  f.registry.byConfig.get('one').shell = { cwd: '/next' }
  await flush()
  assert.equal(f.requests[1].path, '/next')
})

test('transfers capture the listed directory before a picker or terminal changes it', async (t) => {
  const f = fixture(t)
  const enqueued = [], created = []
  let select
  Object.assign(f.context, {
    joinRemote, busy: ref(false), folderName: ref('child'), folderOpen: ref(true),
    emit() {}, t: (key) => key, crypto: { randomUUID: () => 'transfer' },
  })
  Object.assign(f.context.remoteApi, {
    selectPaths: () => new Promise((resolve) => { select = resolve }),
    enqueueTransfer: async (...args) => enqueued.push(args),
    createDirectory: async (...args) => created.push(args),
  })
  const script = source.slice(source.indexOf('async function makeDirectory('), source.indexOf('async function retry('))
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, f.context)
  f.context.pathInput.value = '/unsubmitted-draft'
  const pending = f.run("transfer('upload-files')")
  f.context.directory.value = '/changed'
  select({ token: 'grant' })
  await pending
  assert.equal(enqueued[0][3], '/home')
  await f.run('makeDirectory()')
  assert.deepEqual(created, [['workspace', '/changed/child']])
})
