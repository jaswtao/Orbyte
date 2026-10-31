/** Alt+数字选择 Tab。Author: jaswtao · Date: 2026-10-15 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { loadService } from './load-service.mjs'
import { nextTick, reactive, ref, watch } from 'vue'

const { matchShortcut, bindings, shortcutConflicts, shortcutHint, validShortcut, setLocale } = await loadService('workspace/shortcuts')
const { filterProjects } = await loadService('sidebar/filter')
const { appearanceVariables, palettes, resolveAppearance } = await loadService('appearance')
const tabShortcutIndex = (event) => { const id = matchShortcut(event); return /^tab[1-9]$/.test(id ?? '') ? Number(id.slice(3)) - 1 : -1 }
const key = (code, modifiers = {}) => ({ altKey: false, ctrlKey: false, shiftKey: false, metaKey: false, code, ...modifiers })

test('Changing file search scope clears old results and discards responses from the previous scope', async (t) => {
  const source = await readFile(new URL('../src/components/files/QuickOpen.vue', import.meta.url), 'utf8')
  const ast = ts.createSourceFile('QuickOpen.ts', source.split('<script setup lang="ts">')[1].split('</script>')[0], ts.ScriptTarget.Latest, true)
  const nodes = ast.statements.filter((node) =>
    (ts.isFunctionDeclaration(node) && node.name?.text === 'search') ||
    (ts.isExpressionStatement(node) && ts.isCallExpression(node.expression) && node.expression.expression.getText(ast) === 'watch'))
  const requests = [], timers = new Map()
  let timerId = 0
  const context = vm.createContext({
    props: reactive({ show: false, projectId: 'project', includeIgnored: false }),
    query: ref('file'), paths: ref([]), incomplete: ref(false), busy: ref(false), selected: ref(0), input: ref(),
    timer: undefined, generation: 0, nextTick,
    watch: (...args) => { const stop = watch(...args); t.after(stop); return stop },
    setTimeout: (fn) => { timers.set(++timerId, fn); return timerId }, clearTimeout: (id) => timers.delete(id),
    fileApi: { search: (...args) => new Promise((resolve) => requests.push({ args, resolve })) },
    emit: () => assert.fail('Unexpected search error'), fileError: (reason) => reason,
  })
  vm.runInContext(ts.transpileModule(nodes.map((node) => node.getText(ast)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  const runTimer = () => { const pending = [...timers.values()]; timers.clear(); pending.forEach((fn) => fn()) }
  context.props.show = true
  await nextTick()
  runTimer()
  assert.deepEqual(requests[0].args, ['project', 'file', false])
  requests[0].resolve({ paths: ['file.txt'], incomplete: false })
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(Array.from(context.paths.value), ['file.txt'])
  context.props.includeIgnored = true
  await nextTick()
  assert.deepEqual(Array.from(context.paths.value), [])
  runTimer()
  assert.deepEqual(requests[1].args, ['project', 'file', true])
  context.props.includeIgnored = false
  await nextTick()
  runTimer()
  requests[1].resolve({ paths: ['ignored/file.txt'], incomplete: true })
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(Array.from(context.paths.value), [])
  assert.equal(context.incomplete.value, false)
  requests[2].resolve({ paths: ['file.txt'], incomplete: false })
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(Array.from(context.paths.value), ['file.txt'])
  context.props.show = false
  await nextTick()
  assert.deepEqual(Array.from(context.paths.value), [])
  assert.equal(timers.size, 0)
})

test('File shortcuts select separate commands and respect custom or disabled bindings', () => {
  const custom = key('KeyO', { ctrlKey: true })
  for (const shiftKey of [false, true]) {
    const event = key('KeyP', { ctrlKey: true, shiftKey })
    const command = shiftKey ? 'quickOpen' : 'openFile'
    assert.equal(matchShortcut(event), command)
    assert.equal(matchShortcut(event, {}, ['workspace']), command)
    assert.equal(matchShortcut(event, { [command]: [custom] }), undefined)
    assert.equal(matchShortcut(event, { [command]: [] }), undefined)
    assert.equal(matchShortcut(custom, { [command]: [custom] }), command)
    assert.ok(shortcutConflicts({ newTerminal: [event] }).length)
    assert.equal(matchShortcut({ ...event, isComposing: true }), undefined)
  }
  assert.equal(matchShortcut(custom, { quickOpen: [custom] }), 'quickOpen')
  assert.equal(shortcutHint('openFile'), 'Ctrl+P')
  assert.equal(shortcutHint('quickOpen'), 'Ctrl+Shift+P')
  assert.deepEqual(shortcutConflicts({}), [])
  const migrated = { quickOpen: [key('KeyP', { ctrlKey: true })], openFile: [] }
  assert.deepEqual(shortcutConflicts(migrated), [])
  assert.equal(matchShortcut(key('KeyP', { ctrlKey: true }), migrated), 'quickOpen')
  assert.equal(shortcutHint('openFile', migrated), '')
})

const appSource = await readFile(new URL('../src/App.vue', import.meta.url), 'utf8')
const appScript = appSource.split('<script setup lang="ts">')[1].split('</script>')[0]
const appAst = ts.createSourceFile('App.ts', appScript, ts.ScriptTarget.Latest, true)
const closeShortcut = appAst.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === 'closeShortcut')
const closeShortcutJs = ts.transpileModule(closeShortcut.getText(appAst), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

function appCloseFixture(close = async () => {}) {
  const calls = [], errors = []
  const context = vm.createContext({
    closeRequestPending: false, disposed: false,
    closeDialog: { value: false }, closeBusy: { value: false },
    getCurrentWindow: () => ({ async close() { calls.push('close'); await close() } }),
    reportError: (message) => errors.push(message),
    t: (id, params) => `${id}: ${params.reason}`,
  })
  vm.runInContext(closeShortcutJs, context)
  return { context, calls, errors, async press(modifiers = {}, code = 'F4') {
    const event = { ...key(code, { altKey: true, ...modifiers }),
      preventDefault() { this.prevented = true }, stopImmediatePropagation() { this.stopped = true } }
    await context.closeShortcut(event)
    return event
  } }
}

test('Alt+F4 requests window close and intercepts the event before terminal or editor input', async () => {
  const f = appCloseFixture()
  const event = await f.press()
  assert.deepEqual(f.calls, ['close'])
  assert.equal(event.prevented, true)
  assert.equal(event.stopped, true)
  for (const modifiers of [{ altKey: false }, { ctrlKey: true }, { shiftKey: true }, { metaKey: true }]) {
    const unmatched = await f.press(modifiers)
    assert.equal(unmatched.prevented, undefined)
    assert.equal(unmatched.stopped, undefined)
  }
  await f.press({}, 'F3')
  assert.deepEqual(f.calls, ['close'])
})

test('Alt+F4 suppresses repeats, composition, pending requests and close confirmation', async () => {
  let finishClose
  const f = appCloseFixture(() => new Promise((resolve) => { finishClose = resolve }))
  for (const modifiers of [{ repeat: true }, { isComposing: true }]) {
    const event = await f.press(modifiers)
    assert.equal(event.prevented, true)
    assert.equal(event.stopped, true)
  }
  const pending = f.press()
  await f.press()
  assert.deepEqual(f.calls, ['close'])
  finishClose()
  await pending
  assert.equal(f.context.closeRequestPending, false)
  for (const state of ['closeDialog', 'closeBusy']) {
    f.context[state].value = true
    await f.press()
    f.context[state].value = false
  }
  f.context.disposed = true
  await f.press()
  assert.deepEqual(f.calls, ['close'])
})

test('Alt+F4 reports close failures and allows retry after failure or cancelled confirmation', async () => {
  let fail = true
  const f = appCloseFixture(async () => { if (fail) throw new Error('close failed') })
  await f.press()
  assert.deepEqual(f.errors, ['titlebar.windowActionFailed: Error: close failed'])
  assert.equal(f.context.closeRequestPending, false)
  fail = false
  await f.press()
  f.context.closeDialog.value = true
  await f.press()
  f.context.closeDialog.value = false
  await f.press()
  assert.deepEqual(f.calls, ['close', 'close', 'close'])
})

function listenerCalls(handler) {
  const found = []
  const visit = (node) => {
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression)
      && ['addEventListener', 'removeEventListener'].includes(node.expression.name.text)
      && node.arguments[1]?.getText(appAst) === handler) {
      const inside = []
      for (let current = node.parent; current; current = current.parent) {
        if (ts.isCallExpression(current) && ts.isIdentifier(current.expression)) inside.push(current.expression.text)
      }
      found.push({ method: node.expression.name.text, target: node.expression.expression.getText(appAst), args: node.arguments.map((arg) => arg.getText(appAst)), inside })
    }
    ts.forEachChild(node, visit)
  }
  visit(appAst)
  return found
}

test('Alt+F4 listener is registered on document during setup and removed on unmount', () => {
  assert.deepEqual(listenerCalls('closeShortcut'), [
    { method: 'addEventListener', target: 'document', args: ["'keydown'", 'closeShortcut', 'true'], inside: [] },
    { method: 'removeEventListener', target: 'document', args: ["'keydown'", 'closeShortcut', 'true'], inside: ['onBeforeUnmount'] },
  ])
})

function t(key, params = {}) {
  const messages = {
    'workspace.unsavedSuffix': '{text} {count} file(s) have unsaved changes.',
    'workspace.closeTab': 'Closing “{title}” stops every terminal in it and their managed tasks. Continue?',
    'workspace.closePane': 'Closing this pane stops the terminal and its managed tasks. Continue?',
    'workspace.closeEditor': 'Close this editor?',
    'workspace.stopTerminal': 'Stop this terminal and its managed tasks? The configuration and pane stay.',
    'workspace.taskCheckFailed': 'Could not check running terminal tasks, so close confirmation is required: {reason}',
    'workspace.saveIncomplete': 'A file is still unsaved. Resolve the save error or conflict first.',
  }
  return (messages[key] ?? key).replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? ''))
}

// 运行真实弹窗脚本，以模拟 IPC 验证两个设置草稿的保存边界。
async function settingsDialogFixture(name, store, projectApi) {
  const source = await readFile(new URL(`../src/components/${name}.vue`, import.meta.url), 'utf8')
  const script = source.split('<script setup lang="ts">')[1].split('</script>')[0]
  const ast = ts.createSourceFile(`${name}.ts`, script, ts.ScriptTarget.Latest, true)
  const body = ast.statements.filter((node) => !ts.isImportDeclaration(node)).map((node) => node.getText(ast)).join('\n')
  const watchers = []
  const emitted = []
  const props = { show: false }
  const context = vm.createContext({
    defineProps: () => props, defineEmits: () => (...args) => emitted.push(args),
    useProjects: () => store, projectApi, terminalApi: { listAvailableShells: async () => [] },
    shortcutConflicts, t, shellLabel: (shell) => shell.description, ref: (value) => ({ value }), reactive: (value) => value,
    useOsTheme: () => ({ value: 'dark' }), appearanceVariables, palettes, resolveAppearance,
    computed: (options) => ({
      get value() { return typeof options === 'function' ? options() : options.get() },
      set value(value) { options.set(value) },
    }), watch: (_, callback) => watchers.push(callback),
  })
  vm.runInContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  return {
    emitted,
    evaluate: (code) => vm.runInContext(code, context),
    async show(value = true) { props.show = value; for (const callback of watchers) await callback(value) },
  }
}

function settingsStoreFixture() {
  return {
    settings: { shortcuts: {}, theme: 'dark', workspace_border_color: null, workspace_border_intensity: 100, font_family: 'monospace', font_size: 14, scrollback: 5000,
      default_shell: 'powershell', shell_integration: true, confirm_close_terminal: true, notify_command_seconds: 30, default_directory: null },
    busy: false, error: '',
    async run(action) {
      if (this.busy) return false
      this.busy = true
      try { await action(); return true }
      catch (error) { this.error = String(error); return false }
      finally { this.busy = false }
    },
  }
}

test('快捷键独立保存，主设置保存采用最新绑定且保留其他未保存草稿', async () => {
  const store = settingsStoreFixture()
  const api = {
    async setSetting(name, value) { store.settings = { ...store.settings, [name]: structuredClone(value) } },
    async setSettings(value) { store.settings = { ...store.settings, ...structuredClone(value) } },
  }
  const main = await settingsDialogFixture('SettingsDialog', store, api)
  const child = await settingsDialogFixture('ShortcutSettingsDialog', store, api)
  await main.show()
  main.evaluate('draft.font_size = 20')
  main.evaluate("borderColor.value = '#AABBCC'")
  assert.equal(main.evaluate("borderPreview.value['--workspace-border']"), '#AABBCC')
  await child.show()
  child.evaluate('draft.value = { newTerminal: [] }')
  await child.evaluate('save()')
  assert.deepEqual(store.settings.shortcuts, { newTerminal: [] })
  assert.equal(store.settings.font_size, 14)
  assert.equal(store.settings.workspace_border_color, null)
  assert.deepEqual(child.emitted, [['update:show', false]])
  await main.evaluate('save()')
  assert.deepEqual(store.settings.shortcuts, { newTerminal: [] })
  assert.equal(store.settings.font_size, 20)
  assert.equal(store.settings.workspace_border_color, '#AABBCC')
})

test('关闭确认设置保存后生效，取消编辑并重新打开恢复已保存值', async () => {
  const store = settingsStoreFixture()
  const main = await settingsDialogFixture('SettingsDialog', store, {
    async setSettings(value) { store.settings = { ...store.settings, ...structuredClone(value) } },
  })
  await main.show()
  assert.equal(main.evaluate('draft.confirm_close_terminal'), true)
  main.evaluate('draft.confirm_close_terminal = false')
  await main.show(false)
  assert.equal(store.settings.confirm_close_terminal, true)
  await main.show()
  assert.equal(main.evaluate('draft.confirm_close_terminal'), true)
  main.evaluate('draft.confirm_close_terminal = false')
  await main.evaluate('save()')
  assert.equal(store.settings.confirm_close_terminal, false)
  await main.show()
  assert.equal(main.evaluate('draft.confirm_close_terminal'), false)
})

test('取消快捷键修改不写入配置，重新打开从已保存配置创建独立草稿', async () => {
  const store = settingsStoreFixture()
  store.settings.shortcuts = { newTerminal: [key('KeyQ', { ctrlKey: true })] }
  const child = await settingsDialogFixture('ShortcutSettingsDialog', store, {})
  await child.show()
  child.evaluate('draft.value.newTerminal[0].code = "KeyY"')
  await child.show(false)
  assert.equal(store.settings.shortcuts.newTerminal[0].code, 'KeyQ')
  await child.show()
  assert.equal(child.evaluate('draft.value.newTerminal[0].code'), 'KeyQ')
})

test('快捷键冲突阻止提交，保存失败保留草稿并允许重试，保存期间阻止重复提交', async () => {
  const store = settingsStoreFixture()
  let attempts = 0
  let finish
  const child = await settingsDialogFixture('ShortcutSettingsDialog', store, {
    async setSetting(name, value) {
      attempts++
      if (attempts === 1) throw new Error('写入失败')
      await new Promise((resolve) => { finish = resolve })
      store.settings[name] = structuredClone(value)
    },
  })
  await child.show()
  child.evaluate(`draft.value = { newTerminal: ${JSON.stringify(bindings('quickOpen'))} }`)
  await child.evaluate('save()')
  assert.equal(attempts, 0)
  child.evaluate('draft.value = { newTerminal: [] }')
  await child.evaluate('save()')
  assert.match(child.evaluate('error.value'), /写入失败/)
  assert.equal(child.evaluate('draft.value.newTerminal.length'), 0)
  assert.equal(child.emitted.length, 0)
  const pending = child.evaluate('save()')
  assert.equal(child.evaluate('saving.value'), true)
  await child.evaluate('save()')
  assert.equal(attempts, 2)
  finish()
  await pending
  assert.equal(child.evaluate('saving.value'), false)
  assert.deepEqual(store.settings.shortcuts, { newTerminal: [] })
  assert.deepEqual(child.emitted, [['update:show', false]])
})

const script = appSource.split('<script setup lang="ts">')[1].split('</script>')[0]
const ast = ts.createSourceFile('App.ts', script, ts.ScriptTarget.Latest, true)
const handler = ast.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === 'workspaceShortcut')
const handlerJs = ts.transpileModule(handler.getText(ast), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

function projectNavigation(visible) {
  const selected = [], revealed = []
  // 原始数组顺序与分组显示顺序故意不同。
  const projects = [{ id: 'b', groupId: 'second' }, { id: 'u', groupId: null }, { id: 'a', groupId: 'first' }, { id: 'c', groupId: 'second' }]
  const store = { projects, groups: [{ id: 'first' }, { id: 'second' }], active: projects[2], activate: (id) => { selected.push(id); return true } }
  const context = { matchShortcut, filterProjects, store, nextTick, disposed: false, sidebarView: { value: 'projects' }, sidebarVisible: false, setSidebarVisible(value) { context.sidebarVisible = value }, sidebar: { value: visible === undefined ? undefined : { revealProject: (id) => revealed.push(id), getNavigationProjects: () => visible.map((id) => projects.find((p) => p.id === id)) } } }
  for (const name of ['renamingProject', 'projectDialog', 'groupDialog', 'deleting', 'settingsDialog', 'closeDialog', 'worktreeDialog', 'recoveryDialog']) context[name] = { value: false }
  vm.createContext(context)
  vm.runInContext(handlerJs, context)
  return { store, selected, revealed, context, press(key, overrides = {}) { return context.workspaceShortcut({ key, code: key, ctrlKey: true, shiftKey: true, altKey: false, metaKey: false, repeat: false, preventDefault() {}, stopPropagation() {}, ...overrides }) } }
}

test('上下键按侧栏提供的可见顺序切换，跳过隐藏项目并首尾循环', async () => {
  const nav = projectNavigation(['a', 'c', 'u'])
  await nav.press('ArrowDown')
  await nav.press('ArrowUp')
  nav.store.active = { id: 'u' }
  await nav.press('ArrowDown')
  assert.deepEqual(nav.selected, ['c', 'u', 'a'])
})

test('侧栏未挂载时按分组顺序切换，未分组排最后', async () => {
  const nav = projectNavigation()
  await nav.press('ArrowDown')
  await nav.press('ArrowUp')
  assert.deepEqual(nav.selected, ['b', 'u'])
})

test('可见列表为空不切换，当前项目隐藏时下键从首项、上键从末项开始', async () => {
  const empty = projectNavigation([])
  await empty.press('ArrowDown')
  assert.deepEqual(empty.selected, [])
  const nav = projectNavigation(['c', 'u'])
  await nav.press('ArrowDown')
  await nav.press('ArrowUp')
  assert.deepEqual(nav.selected, ['c', 'u'])
})

test('左右键保留原始顺序，上下键忽略长按和额外修饰键', async () => {
  const nav = projectNavigation(['a', 'u'])
  await nav.press('ArrowLeft')
  await nav.press('ArrowRight')
  await nav.press('ArrowDown', { repeat: true })
  await nav.press('ArrowDown', { altKey: true })
  assert.deepEqual(nav.selected, ['u', 'c'])
})

test('侧栏未挂载时所有项目切换快捷键遵守隐藏开关', async () => {
  const nav = projectNavigation()
  nav.store.projects.find((p) => p.id === 'b').hidden = true
  nav.store.projects.find((p) => p.id === 'c').hidden = true
  await nav.press('ArrowDown')
  await nav.press('ArrowRight')
  assert.deepEqual(nav.selected, ['u', 'u'])
  nav.store.settings = { show_hidden_projects: true }
  await nav.press('ArrowDown')
  await nav.press('ArrowRight')
  assert.deepEqual(nav.selected, ['u', 'u', 'b', 'c'])
})

test('上下切换先展示并恢复侧栏，再读取导航顺序，激活成功后定位', async () => {
  const nav = projectNavigation(['a', 'c', 'u'])
  const view = nav.context.sidebar.value
  nav.context.sidebar.value = undefined
  nav.context.sidebarView.value = 'files'
  nav.context.nextTick = async () => {
    assert.equal(nav.context.sidebarVisible, true)
    assert.equal(nav.context.sidebarView.value, 'projects')
    nav.context.sidebar.value = view
  }
  nav.store.activate = async (id) => {
    nav.selected.push(id)
    nav.store.active = nav.store.projects.find((project) => project.id === id)
    assert.deepEqual(nav.revealed, [])
    return true
  }
  await nav.press('ArrowDown')
  assert.deepEqual(nav.selected, ['c'])
  assert.deepEqual(nav.revealed, ['c'])
})

test('首尾循环后定位当前激活项，左右键保持侧栏状态', async () => {
  const nav = projectNavigation(['a', 'c', 'u'])
  nav.store.activate = async (id) => { nav.store.active = { id }; return true }
  await nav.press('ArrowUp')
  await nav.press('ArrowDown')
  assert.deepEqual(nav.revealed, ['u', 'a'])
  nav.context.sidebarVisible = false
  nav.context.sidebarView.value = 'files'
  await nav.press('ArrowRight')
  assert.equal(nav.context.sidebarVisible, false)
  assert.equal(nav.context.sidebarView.value, 'files')
  assert.deepEqual(nav.revealed, ['u', 'a'])
})

test('激活失败、忙碌或无候选时不定位，等待渲染时再次忙碌不激活', async () => {
  const failed = projectNavigation(['a', 'c'])
  failed.store.activate = async () => false
  await failed.press('ArrowDown')
  assert.deepEqual(failed.revealed, [])
  const busy = projectNavigation(['a', 'c'])
  busy.store.busy = true
  await busy.press('ArrowDown')
  assert.deepEqual(busy.selected, [])
  assert.equal(busy.context.sidebarVisible, false)
  busy.store.busy = false
  busy.context.nextTick = async () => { busy.store.busy = true }
  await busy.press('ArrowDown')
  assert.deepEqual(busy.selected, [])
  const empty = projectNavigation([])
  await empty.press('ArrowDown')
  assert.deepEqual(empty.revealed, [])
})

const clampJs = ts.transpileModule(
  ast.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === 'clampSidebarWidth').getText(ast),
  { compilerOptions: { target: ts.ScriptTarget.ES2022 } },
).outputText
const persistJs = ts.transpileModule(
  ast.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === 'persistSidebarWidth').getText(ast),
  { compilerOptions: { target: ts.ScriptTarget.ES2022 } },
).outputText

function sidebarResize(innerWidth = 1200) {
  const selected = []
  const stored = new Map()
  const projects = [{ id: 'a' }, { id: 'b' }]
  const sidebarWidth = { value: 280 }
  const store = { projects, groups: [], active: projects[0], settings: { shortcuts: {} }, activate: (id) => { selected.push(id); store.active = projects.find((project) => project.id === id) } }
  const context = {
    matchShortcut, filterProjects, store, sidebarWidth, sidebarView: { value: 'projects' }, sidebar: { value: undefined },
    SIDEBAR_MIN: 220, SIDEBAR_MAX: 640, window: { innerWidth },
    localStorage: { setItem: (name, value) => stored.set(name, value) },
  }
  for (const name of ['renamingProject', 'projectDialog', 'groupDialog', 'deleting', 'settingsDialog', 'closeDialog', 'worktreeDialog', 'recoveryDialog']) context[name] = { value: false }
  vm.createContext(context)
  vm.runInContext(clampJs + persistJs + handlerJs, context)
  return {
    sidebarWidth, stored, selected, context,
    press(code, overrides = {}) {
      let prevented = false
      context.workspaceShortcut({
        key: code, code, ctrlKey: true, shiftKey: true, altKey: true, metaKey: false, repeat: false,
        preventDefault() { prevented = true }, stopPropagation() {}, ...overrides,
      })
      return prevented
    },
  }
}

test('Ctrl+Shift+Alt+左右键按 16px 调整侧栏宽度，长按连续调整并记住宽度', () => {
  const resize = sidebarResize()
  assert.equal(resize.press('ArrowLeft'), true)
  assert.equal(resize.sidebarWidth.value, 264)
  assert.equal(resize.press('ArrowRight', { repeat: true }), true)
  assert.equal(resize.sidebarWidth.value, 280)
  assert.equal(resize.stored.get('orbyte.sidebar-width'), '280')
  assert.deepEqual(resize.selected, [])
})

test('侧栏宽度到达上下限后不再越界', () => {
  const narrow = sidebarResize()
  narrow.sidebarWidth.value = 220
  narrow.press('ArrowLeft')
  assert.equal(narrow.sidebarWidth.value, 220)
  const wide = sidebarResize()
  wide.sidebarWidth.value = 640
  wide.press('ArrowRight')
  assert.equal(wide.sidebarWidth.value, 640)
  const limited = sidebarResize(800)
  limited.sidebarWidth.value = 510
  limited.press('ArrowRight')
  assert.equal(limited.sidebarWidth.value, 520)
})

test('弹窗打开时不调整侧栏，无 Alt 的左右键仍只切换项目', () => {
  const blocked = sidebarResize()
  blocked.context.settingsDialog.value = true
  assert.equal(blocked.press('ArrowLeft'), false)
  assert.equal(blocked.sidebarWidth.value, 280)
  assert.equal(blocked.stored.size, 0)
  const nav = sidebarResize()
  assert.equal(nav.press('ArrowLeft', { altKey: false }), true)
  assert.equal(nav.press('ArrowRight', { altKey: false }), true)
  assert.equal(nav.sidebarWidth.value, 280)
  assert.deepEqual(nav.selected, ['b', 'a'])
})

test('Alt+1 到 Alt+9 对应第 1 到第 9 个 Tab', () => {
  for (let n = 1; n <= 9; n++) assert.equal(tabShortcutIndex(key(`Digit${n}`, { altKey: true })), n - 1)
})

test('缺少 Alt、叠了其他修饰键、数字小键盘或非数字键都不切换', () => {
  assert.equal(tabShortcutIndex(key('Digit1')), -1)
  assert.equal(tabShortcutIndex(key('Digit1', { altKey: true, ctrlKey: true })), -1)
  assert.equal(tabShortcutIndex(key('Digit1', { altKey: true, shiftKey: true })), -1)
  assert.equal(tabShortcutIndex(key('Digit1', { altKey: true, metaKey: true })), -1)
  assert.equal(tabShortcutIndex(key('Digit0', { altKey: true })), -1)
  assert.equal(tabShortcutIndex(key('Numpad1', { altKey: true })), -1)
  assert.equal(tabShortcutIndex(key('ArrowLeft', { altKey: true })), -1)
})


test('覆盖替换全部旧绑定，清除禁用，删除覆盖恢复默认，提示随配置变化', () => {
  const custom = key('KeyQ', { ctrlKey: true, altKey: true })
  const overrides = { nextTab: [custom] }
  assert.equal(matchShortcut(custom, overrides), 'nextTab')
  for (const old of bindings('nextTab')) assert.notEqual(matchShortcut(old, overrides), 'nextTab')
  assert.equal(shortcutHint('nextTab', overrides), 'Ctrl+Alt+Q')
  assert.equal(matchShortcut(custom, { nextTab: [] }), undefined)
  assert.equal(shortcutHint('nextTab', { nextTab: [] }), '')
  delete overrides.nextTab
  assert.equal(matchShortcut(key('Tab', { ctrlKey: true }), overrides), 'nextTab')
})

test('精确匹配物理键位和修饰键，输入法组词不触发，F 键可单独绑定', () => {
  const custom = key('KeyQ', { ctrlKey: true })
  const overrides = { newTerminal: [custom] }
  assert.equal(matchShortcut({ ...custom, key: '别的字符' }, overrides), 'newTerminal')
  assert.equal(matchShortcut({ ...custom, shiftKey: true }, overrides), undefined)
  assert.equal(matchShortcut({ ...custom, isComposing: true }, overrides), undefined)
  assert.equal(validShortcut(key('KeyQ')), false)
  assert.equal(validShortcut(key('KeyQ', { shiftKey: true })), false)
  assert.equal(validShortcut(key('F12')), true)
  assert.equal(validShortcut(key('F13')), false)
})

test('冲突检查包含未覆盖默认值，区分终端与编辑器，并拒绝重复绑定', () => {
  const custom = key('KeyQ', { ctrlKey: true })
  assert.deepEqual(shortcutConflicts({}), [])
  assert.deepEqual(shortcutConflicts({ terminalCopy: [custom], editorSave: [custom] }), [])
  assert.ok(shortcutConflicts({ newTerminal: [custom], editorSave: [custom] }).length)
  assert.ok(shortcutConflicts({ newTerminal: bindings('quickOpen') }).some((error) => error.includes('Quick open file')))
  assert.ok(shortcutConflicts({ newTerminal: [custom, custom] }).some((error) => error.includes('Duplicate binding')))
  setLocale('zh')
  try {
    assert.ok(shortcutConflicts({ newTerminal: bindings('quickOpen') }).some((error) => error.includes('快速打开文件')))
    assert.ok(shortcutConflicts({ newTerminal: [custom, custom] }).some((error) => error.includes('绑定重复')))
  } finally { setLocale('en') }
  assert.equal(matchShortcut(custom, { terminalCopy: [custom], editorSave: [custom] }, ['editor']), 'editorSave')
})

test('项目自定义保存后读取最新组合，旧组合与组词事件不切换项目', async () => {
  const nav = projectNavigation(['a', 'u'])
  nav.store.settings = { shortcuts: { projectDown: [key('KeyN', { altKey: true })] } }
  await nav.press('ArrowDown')
  await nav.press('n', { code: 'KeyN', ctrlKey: false, shiftKey: false, altKey: true, isComposing: true })
  assert.deepEqual(nav.selected, [])
  await nav.press('n', { code: 'KeyN', ctrlKey: false, shiftKey: false, altKey: true })
  assert.deepEqual(nav.selected, ['u'])
})

const settingsSource = await readFile(new URL('../src/components/ShortcutSettings.vue', import.meta.url), 'utf8')
const settingsScript = settingsSource.split('<script setup lang="ts">')[1].split('</script>')[0]
const settingsAst = ts.createSourceFile('ShortcutSettings.ts', settingsScript, ts.ScriptTarget.Latest, true)
const settingsFunctions = settingsAst.statements.filter((node) => ts.isFunctionDeclaration(node)).map((node) => node.getText(settingsAst)).join('\n')
const settingsJs = ts.transpileModule(settingsFunctions, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
function settingsFixture() {
  const original = { newTerminal: [key('KeyQ', { ctrlKey: true })] }
  const context = { bindings, validShortcut, t: (key) => key, props: { active: true, disabled: false, modelValue: structuredClone(original) }, recording: { value: null }, message: { value: '' } }
  context.emit = (_, value) => { context.props.modelValue = value }
  vm.createContext(context)
  vm.runInContext(settingsJs, context)
  return { context, original, press(code, modifiers = {}) {
    const event = { ...key(code, modifiers), key: code, preventDefault() { this.prevented = true }, stopImmediatePropagation() { this.stopped = true } }
    context.capture(event)
    return event
  } }
}

test('录入吞掉按键，拒绝裸字母；Esc 只取消录入，恢复操作不修改原配置', () => {
  const { context, original, press } = settingsFixture()
  context.recording.value = { id: 'newTerminal', index: 0 }
  const invalid = press('KeyA')
  assert.equal(invalid.prevented, true)
  assert.equal(invalid.stopped, true)
  assert.ok(context.message.value)
  press('Escape')
  assert.equal(context.recording.value, null)
  assert.equal(context.props.modelValue.newTerminal[0].code, 'KeyQ')
  context.recording.value = { id: 'newTerminal', index: 0 }
  press('KeyN', { ctrlKey: true })
  assert.equal(context.props.modelValue.newTerminal[0].code, 'KeyN')
  assert.equal(original.newTerminal[0].code, 'KeyQ')
  context.restore('newTerminal')
  assert.equal(context.props.modelValue.newTerminal, undefined)
  context.update('newTerminal', [])
  context.restore()
  assert.equal(Object.keys(context.props.modelValue).length, 0)
})


const editorSource = await readFile(new URL('../src/services/editor/registry.ts', import.meta.url), 'utf8')
const editorAst = ts.createSourceFile('EditorRegistry.ts', editorSource, ts.ScriptTarget.Latest, true)
const editorClass = editorAst.statements.find((node) => ts.isClassDeclaration(node))
const saveMethod = editorClass.members.find((node) => node.name?.getText(editorAst) === 'saveShortcut')
const editorJs = ts.transpileModule(`class EditorFixture { ${saveMethod.getText(editorAst)} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

test('已有编辑器保存处理读取最新设置，旧组合、组词、弹窗和长按不重复保存', () => {
  const context = { matchShortcut }
  vm.createContext(context)
  vm.runInContext(editorJs + '; globalThis.fixture = new EditorFixture()', context)
  const calls = []
  const settings = { shortcuts: {} }
  let blocked = false
  context.fixture.settings = () => settings
  context.fixture.shortcutsBlocked = () => blocked
  context.fixture.save = (id) => calls.push(id)
  const press = (code, modifiers = {}) => context.fixture.saveShortcut('file', { ...key(code, modifiers), preventDefault() {} })
  assert.equal(press('KeyS', { ctrlKey: true }), true)
  settings.shortcuts = { editorSave: [key('KeyQ', { ctrlKey: true })] }
  assert.equal(press('KeyS', { ctrlKey: true }), false)
  assert.equal(press('KeyQ', { ctrlKey: true }), true)
  assert.equal(press('KeyQ', { ctrlKey: true, repeat: true }), true)
  assert.equal(press('KeyQ', { ctrlKey: true, isComposing: true }), false)
  blocked = true
  assert.equal(press('KeyQ', { ctrlKey: true }), false)
  assert.deepEqual(calls, ['file', 'file'])
})

const workspaceSource = await readFile(new URL('../src/components/workspace/WorkspaceView.vue', import.meta.url), 'utf8')
const workspaceScript = workspaceSource.split('<script setup lang="ts">')[1].split('</script>')[0]
const workspaceAst = ts.createSourceFile('WorkspaceView.ts', workspaceScript, ts.ScriptTarget.Latest, true)
const workspaceHandler = workspaceAst.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === 'keydown')
const workspaceJs = ts.transpileModule(workspaceHandler.getText(workspaceAst), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

test('File shortcuts pass the search scope to the picker and prevent default input', () => {
  let opened = 0
  let selectedMode
  const context = vm.createContext({ matchShortcut,
    props: { active: true, shortcutsBlocked: false, settings: { shortcuts: {} } },
    tabMenu: { value: null }, confirmation: { value: null }, rename: { value: null }, quickOpen: { value: false },
    chooseFile(direction, withIgnored) { opened++; selectedMode = withIgnored; context.quickOpen.value = true },
  })
  vm.runInContext(workspaceJs, context)
  for (const shiftKey of [false, true]) {
    context.quickOpen.value = false
    const event = { ...key('KeyP', { ctrlKey: true, shiftKey }),
      preventDefault() { this.prevented = true }, stopPropagation() { this.stopped = true } }
    const before = opened
    context.keydown({ ...event, repeat: true })
    assert.equal(opened, before)
    context.keydown(event)
    assert.equal(opened, before + 1)
    assert.equal(selectedMode, shiftKey)
    assert.equal(event.prevented, true)
    assert.equal(event.stopped, true)
    context.keydown(event)
    assert.equal(opened, before + 1)
    context.quickOpen.value = false
    context.props.active = false
    context.keydown(event)
    context.props.active = true
    context.props.shortcutsBlocked = true
    context.keydown(event)
    context.props.shortcutsBlocked = false
    assert.equal(opened, before + 1)
  }
})
const { leaves } = await loadService('workspace/layout')

test('关闭页签默认 Ctrl+F4，精确匹配修饰键并支持覆盖、禁用和冲突校验', () => {
  const close = key('F4', { ctrlKey: true })
  assert.equal(matchShortcut(close), 'closeTab')
  assert.equal(shortcutHint('closeTab'), 'Ctrl+F4')
  for (const modifiers of [{}, { ctrlKey: true, altKey: true }, { ctrlKey: true, shiftKey: true }, { ctrlKey: true, metaKey: true }]) {
    assert.notEqual(matchShortcut(key('F4', modifiers)), 'closeTab')
  }
  const custom = key('KeyQ', { ctrlKey: true })
  assert.equal(matchShortcut(custom, { closeTab: [custom] }), 'closeTab')
  assert.equal(matchShortcut(close, { closeTab: [custom] }), undefined)
  assert.equal(matchShortcut(close, { closeTab: [] }), undefined)
  assert.deepEqual(shortcutConflicts({}), [])
  assert.ok(shortcutConflicts({ newTerminal: [close] }).some((error) => error.includes('Close current tab')))
})

test('Ctrl+F4 关闭激活的整个分屏页签，保留关闭确认并阻断长按、后台和弹窗', () => {
  const terminal = { type: 'terminal', terminalConfigId: 'terminal' }
  const editor = { type: 'editor', id: 'editor', path: 'file.txt' }
  const tab = { id: 'active', title: '分屏', root: { type: 'split', children: [terminal, editor] } }
  const calls = []
  const context = {
    matchShortcut, leaves, disposed: false,
    props: { active: true, shortcutsBlocked: false, settings: { shortcuts: {}, confirm_close_terminal: true },
      editors: { entries: new Map([['editor', { document: { dirty: true } }]]) },
      registry: { byConfig: new Map([['terminal', { status: { state: 'running' } }]]) } },
    activeTab: { value: tab }, busy: { value: false },
    perform: (action) => action(), closeIds: (ids, selected) => calls.push({ ids: [...ids], tab: selected }),
    t,
  }
  for (const name of ['tabMenu', 'confirmation', 'rename', 'quickOpen']) context[name] = { value: null }
  vm.createContext(context)
  const closeFunctions = workspaceAst.statements.filter((node) => ts.isFunctionDeclaration(node) && ['closeTab', 'confirmIfRunning'].includes(node.name?.text))
  vm.runInContext(ts.transpileModule(closeFunctions.map((node) => node.getText(workspaceAst)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  vm.runInContext(workspaceJs, context)
  let prevented = 0
  let stopped = 0
  const press = (extra = {}) => context.keydown({ ...key('F4', { ctrlKey: true }), preventDefault() { prevented++ }, stopPropagation() { stopped++ }, ...extra })
  press()
  assert.equal(prevented, 1)
  assert.equal(stopped, 1)
  assert.equal(calls.length, 0)
  assert.match(context.confirmation.value.text, /1 file\(s\) have unsaved changes/)
  const confirmation = context.confirmation.value
  press()
  assert.equal(context.confirmation.value, confirmation)
  confirmation.action()
  assert.deepEqual(calls, [{ ids: ['terminal', 'editor'], tab }])
  context.confirmation.value = null
  context.props.editors.entries.get('editor').document.dirty = false
  press()
  assert.ok(context.confirmation.value, '运行中的终端仍需确认')
  context.confirmation.value = null
  context.props.registry.byConfig.get('terminal').status.state = 'stopped'
  press({ repeat: true })
  context.props.active = false
  press()
  context.props.active = true
  context.props.shortcutsBlocked = true
  press()
  context.props.shortcutsBlocked = false
  for (const name of ['rename', 'quickOpen']) {
    context[name].value = true
    press()
    context[name].value = null
  }
  context.activeTab.value = undefined
  press()
  context.activeTab.value = tab
  context.busy.value = true
  press()
  assert.equal(calls.length, 1)
  context.busy.value = false
  press()
  assert.equal(calls.length, 2)
  assert.equal(calls[1].tab, tab)
})

const focusNodes = workspaceAst.statements.filter((node) =>
  (ts.isFunctionDeclaration(node) && ['restoreFocus', 'focusTerminal'].includes(node.name?.text)) ||
  (ts.isExpressionStatement(node) && node.getText(workspaceAst).startsWith('watch(') && node.getText(workspaceAst).includes('restoreFocus()')))
const focusJs = ts.transpileModule(focusNodes.map((node) => node.getText(workspaceAst)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

function workspaceFocusFixture(t) {
  const calls = []
  const terminal = { type: 'terminal', terminalConfigId: 'terminal' }
  const editor = { type: 'editor', id: 'editor', path: 'file.txt' }
  const context = {
    nextTick, leaves, disposed: false, document: { hasFocus: () => true },
    watch: (...args) => { const stop = watch(...args); t.after(stop); return stop },
    props: reactive({ active: false, shortcutsBlocked: false,
      registry: { byConfig: new Map([['terminal', { client: { focus: () => calls.push('terminal') } }]]) },
      editors: { entries: new Map([['editor', { dialog: null, view: { focus: () => calls.push('editor') } }]]) } }),
    ready: ref(true), focused: ref('editor'),
    activeTab: ref({ id: 'active', root: { type: 'split', direction: 'horizontal', ratio: 0.5, children: [terminal, editor] } }),
  }
  for (const name of ['confirmation', 'rename', 'quickOpen', 'tabMenu']) context[name] = ref(null)
  vm.createContext(context)
  vm.runInContext(focusJs, context)
  return { context, calls, async flush() { await nextTick(); await nextTick() } }
}

test('项目激活后恢复分屏中的编辑器或终端输入焦点，不重置已有窗格选择', async (t) => {
  const { context, calls, flush } = workspaceFocusFixture(t)
  context.props.active = true
  assert.deepEqual(calls, [])
  await flush()
  assert.deepEqual(calls, ['editor'])
  context.props.active = false
  await flush()
  context.focused.value = 'terminal'
  context.props.active = true
  await flush()
  assert.deepEqual(calls, ['editor', 'terminal'])
})

test('首次加载等待工作区就绪，失效窗格记录回退到激活 Tab 的首个窗格', async (t) => {
  const { context, calls, flush } = workspaceFocusFixture(t)
  context.ready.value = false
  context.focused.value = 'removed'
  context.props.active = true
  await flush()
  assert.deepEqual(calls, [])
  context.ready.value = true
  await flush()
  assert.deepEqual(calls, ['terminal'])
  assert.equal(context.focused.value, 'terminal')
})

test('无激活 Tab 时不聚焦', async (t) => {
  const { context, calls, flush } = workspaceFocusFixture(t)
  context.activeTab.value = undefined
  context.props.active = true
  await flush()
  assert.deepEqual(calls, [])
})

for (const blocked of ['shortcutsBlocked', 'confirmation', 'rename', 'quickOpen', 'tabMenu', 'editorDialog']) {
  test(`项目激活时保留弹窗焦点：${blocked}`, async (t) => {
    const { context, calls, flush } = workspaceFocusFixture(t)
    if (blocked === 'shortcutsBlocked') context.props.shortcutsBlocked = true
    else if (blocked === 'editorDialog') context.props.editors.entries.get('editor').dialog = { kind: 'conflict' }
    else context[blocked].value = true
    context.props.active = true
    await flush()
    assert.deepEqual(calls, [])
  })
}

for (const cancelled of ['inactive', 'disposed']) {
  test(`等待 DOM 更新期间${cancelled === 'inactive' ? '切走项目' : '卸载组件'}，不抢回焦点`, async (t) => {
    const { context, calls, flush } = workspaceFocusFixture(t)
    context.props.active = true
    // watcher 已执行，但其中等待 nextTick 的聚焦操作尚未执行。
    await nextTick()
    if (cancelled === 'inactive') context.props.active = false
    else context.disposed = true
    await flush()
    assert.deepEqual(calls, [])
  })
}

test('窗口激活优先聚焦当前页终端，即使之前选中编辑器', async (t) => {
  const { context, calls, flush } = workspaceFocusFixture(t)
  context.props.active = true
  await flush()
  calls.length = 0
  context.focusTerminal()
  assert.deepEqual(calls, ['terminal'])
  assert.equal(context.focused.value, 'terminal')
})

test('多终端分屏保留当前选中的终端', async (t) => {
  const { context, calls, flush } = workspaceFocusFixture(t)
  context.props.active = true
  await flush()
  context.activeTab.value.root.children.push({ type: 'terminal', terminalConfigId: 'second' })
  context.props.registry.byConfig.set('second', { client: { focus: () => calls.push('second') } })
  context.focused.value = 'second'
  calls.length = 0
  context.focusTerminal()
  assert.deepEqual(calls, ['second'])
})

test('纯编辑器页面激活时保留原焦点，不选择其他标签页', async (t) => {
  const { context, calls, flush } = workspaceFocusFixture(t)
  context.props.active = true
  await flush()
  context.activeTab.value.root = { type: 'editor', id: 'editor', path: 'file.txt' }
  context.focused.value = 'editor'
  calls.length = 0
  context.focusTerminal()
  assert.deepEqual(calls, [])
  assert.equal(context.focused.value, 'editor')
})

test('首次加载纯编辑器标签页时恢复输入焦点，等待期间窗口失焦则不聚焦', async (t) => {
  for (const hasFocus of [true, false]) {
    const { context, calls, flush } = workspaceFocusFixture(t)
    context.ready.value = false
    context.activeTab.value.root = { type: 'editor', id: 'editor', path: 'file.txt' }
    context.props.active = true
    await flush()
    context.ready.value = true
    await nextTick()
    context.document.hasFocus = () => hasFocus
    await flush()
    assert.deepEqual(calls, hasFocus ? ['editor'] : [])
    assert.equal(context.focused.value, 'editor')
  }
})

test('首次加载就绪时优先终端，等待期间窗口失焦则不聚焦', async (t) => {
  for (const hasFocus of [true, false]) {
    const { context, calls, flush } = workspaceFocusFixture(t)
    context.ready.value = false
    context.props.active = true
    await flush()
    context.ready.value = true
    await nextTick()
    context.document.hasFocus = () => hasFocus
    await flush()
    assert.deepEqual(calls, hasFocus ? ['terminal'] : [])
  }
})

for (const blocked of ['shortcutsBlocked', 'confirmation', 'rename', 'quickOpen', 'tabMenu', 'editorDialog', 'inactive', 'disposed', 'unready', 'blurred']) {
  test(`窗口激活聚焦终端时检查保护条件：${blocked}`, async (t) => {
    const { context, calls, flush } = workspaceFocusFixture(t)
    context.props.active = true
    await flush()
    calls.length = 0
    if (blocked === 'shortcutsBlocked') context.props.shortcutsBlocked = true
    else if (blocked === 'editorDialog') context.props.editors.entries.get('editor').dialog = { kind: 'conflict' }
    else if (blocked === 'inactive') context.props.active = false
    else if (blocked === 'disposed') context.disposed = true
    else if (blocked === 'unready') context.ready.value = false
    else if (blocked === 'blurred') context.document.hasFocus = () => false
    else context[blocked].value = true
    context.focusTerminal()
    assert.deepEqual(calls, [])
  })
}

test('窗口 focus 事件等待 DOM 更新后仅转发当前项目，并保留 Git 刷新', async () => {
  const ast = ts.createSourceFile('App.ts', appSource.split('<script setup lang="ts">')[1].split('</script>')[0], ts.ScriptTarget.Latest, true)
  const handler = ast.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === 'windowFocus')
  const deckSource = await readFile(new URL('../src/components/workspace/WorkspaceDeck.vue', import.meta.url), 'utf8')
  const deckAst = ts.createSourceFile('Deck.ts', deckSource.split('<script setup lang="ts">')[1].split('</script>')[0], ts.ScriptTarget.Latest, true)
  const forward = deckAst.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === 'focusTerminal')
  const calls = []
  const deckContext = vm.createContext({ props: { activeId: 'active' }, views: new Map([
    ['active', { focusTerminal: () => calls.push('active') }],
    ['background', { focusTerminal: () => calls.push('background') }],
  ]) })
  vm.runInContext(ts.transpileModule(forward.getText(deckAst), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, deckContext)
  const context = vm.createContext({ nextTick, disposed: false, document: { hasFocus: () => true },
    store: { active: { id: 'active' } }, git: { schedule: (id) => calls.push(`git:${id}`) },
    deck: { value: { focusTerminal: () => deckContext.focusTerminal() } } })
  vm.runInContext(ts.transpileModule(handler.getText(ast), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  const pending = context.windowFocus()
  assert.deepEqual(calls, ['git:active'])
  await pending
  assert.deepEqual(calls, ['git:active', 'active'])
  for (const cancelled of ['blurred', 'disposed', 'noProject']) {
    calls.length = 0
    context.disposed = false
    context.document.hasFocus = () => true
    deckContext.props.activeId = 'active'
    const pending = context.windowFocus()
    if (cancelled === 'blurred') context.document.hasFocus = () => false
    if (cancelled === 'disposed') context.disposed = true
    if (cancelled === 'noProject') deckContext.props.activeId = null
    await pending
    assert.deepEqual(calls, ['git:active'])
  }
})

test('工作区新绑定只执行一次，保留数字 Tab 定位并阻断后台、弹窗和长按', () => {
  const calls = []
  const context = { matchShortcut, props: { active: true, shortcutsBlocked: false, settings: { shortcuts: { newTerminal: [key('KeyQ', { ctrlKey: true })] } } },
    layout: { value: { tabs: [{ id: 'first' }, { id: 'second' }], activeTabId: 'first' } },
    focused: { value: 'pane' }, create: (direction) => calls.push(direction ?? 'new'), cycleTab: (delta) => calls.push(delta), selectTab: (id) => calls.push(id) }
  for (const name of ['tabMenu', 'confirmation', 'rename', 'quickOpen']) context[name] = { value: null }
  vm.createContext(context)
  vm.runInContext(workspaceJs, context)
  const press = (code, modifiers = {}) => context.keydown({ ...key(code, modifiers), preventDefault() {}, stopPropagation() {} })
  press('KeyT', { ctrlKey: true, shiftKey: true })
  press('KeyQ', { ctrlKey: true })
  press('KeyQ', { ctrlKey: true, repeat: true })
  press('Digit2', { altKey: true })
  press('Tab', { ctrlKey: true })
  press('KeyH', { ctrlKey: true, shiftKey: true })
  context.props.active = false
  press('KeyQ', { ctrlKey: true })
  context.props.active = true
  context.props.shortcutsBlocked = true
  press('KeyQ', { ctrlKey: true })
  assert.deepEqual(calls, ['new', 'second', 1, 'horizontal'])
})

test('草稿恢复弹窗阻止项目导航和侧栏快捷键，关闭后恢复', () => {
  const resize = sidebarResize()
  resize.context.recoveryDialog.value = true
  assert.equal(resize.press('ArrowLeft'), false)
  assert.deepEqual(resize.selected, [])
  resize.context.recoveryDialog.value = false
  assert.equal(resize.press('ArrowLeft'), true)
})
/** Rename interaction regressions. Author: jaswtao · Date: 2026-10-30 */
const renameTrackerSource = await readFile(new URL('../src/services/workspace/renameTarget.ts', import.meta.url), 'utf8')
function renameTrackerFixture() {
  class Element {
    constructor(selectors = [], id) { this.selectors = selectors; this.dataset = { projectId: id } }
    closest(selector) { return selector.split(', ').some((s) => this.selectors.includes(s)) ? this : null }
  }
  const context = vm.createContext({ Element })
  vm.runInContext(ts.transpileModule(renameTrackerSource.replace(/^export /gm, ''), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  return { tracker: context.createRenameTargetTracker(), element: (...args) => new Element(...args) }
}

test('Rename defaults, overrides, disabling and conflicts use the shared shortcut manifest', () => {
  assert.equal(matchShortcut(key('F2')), 'rename')
  assert.equal(shortcutHint('rename'), 'F2')
  assert.equal(matchShortcut(key('F2'), { rename: [] }), undefined)
  const custom = key('KeyR', { ctrlKey: true })
  assert.equal(matchShortcut(custom, { rename: [custom] }), 'rename')
  assert.equal(matchShortcut(key('F2'), { rename: [custom] }), undefined)
  for (const modifier of ['altKey', 'ctrlKey', 'shiftKey', 'metaKey', 'isComposing']) assert.equal(matchShortcut(key('F2', { [modifier]: true })), undefined)
  assert.ok(shortcutConflicts({ newTerminal: [key('F2')] }).length)
  for (const command of ['projectSearch', 'newTerminal', 'terminalCopy', 'editorSave']) {
    const migrated = { [command]: [key('F2')], rename: [] }
    assert.equal(matchShortcut(key('F2'), migrated), command)
    assert.equal(shortcutHint('rename', migrated), '')
    assert.deepEqual(shortcutConflicts(migrated), [])
  }
})

test('Automatic terminal focus preserves the clicked project until explicit workspace interaction', () => {
  const { tracker, element } = renameTrackerFixture()
  const project = element(['.project-row'], 'p')
  const terminal = element(['.workspace', '.xterm', 'textarea'])
  tracker.focusin({ target: terminal })
  assert.equal(tracker.target.kind, 'workspace')
  tracker.pointerdown({ target: project })
  tracker.focusin({ target: terminal })
  tracker.keydown({ key: 'F2', target: terminal }, true)
  assert.equal(tracker.target.id, 'p')
  tracker.keydown({ key: 'Control', target: terminal }, false)
  assert.equal(tracker.target.id, 'p')
  tracker.keydown({ key: 'a', target: terminal }, false)
  assert.equal(tracker.target.kind, 'workspace')
  tracker.pointerdown({ target: project })
  tracker.pointerdown({ target: element(['.workspace-tabs']) })
  assert.equal(tracker.target.kind, 'workspace')
  tracker.pointerdown({ target: element(['.workspace', '.cm-content', '[contenteditable="true"]']) })
  assert.equal(tracker.target.kind, 'workspace')
})

test('Tab focus selects its region; unrelated controls and window blur clear the rename target', () => {
  const { tracker, element } = renameTrackerFixture()
  const project = element(['.project-row'], 'p')
  tracker.pointerdown({ target: project })
  tracker.keydown({ key: 'Tab', target: project }, false)
  tracker.focusin({ target: element(['.workspace']) })
  assert.equal(tracker.target.kind, 'workspace')
  tracker.focusin({ target: project })
  assert.equal(tracker.target.id, 'p')
  for (const selectors of [['input'], ['.workspace', '.cm-editor', 'input'], ['.n-modal-container', 'input'], ['.n-dropdown-menu'], []]) {
    tracker.pointerdown({ target: project })
    tracker.focusin({ target: element(selectors) })
    assert.equal(tracker.target, null)
  }
  tracker.pointerdown({ target: project })
  tracker.reset()
  assert.equal(tracker.target, null)
})

function compileFunctions(source, names) {
  const script = source.includes('<script setup lang="ts">') ? source.split('<script setup lang="ts">')[1].split('</script>')[0] : source
  const ast = ts.createSourceFile('fixture.ts', script, ts.ScriptTarget.Latest, true)
  return ts.transpileModule(ast.statements.filter((n) => ts.isFunctionDeclaration(n) && names.includes(n.name?.text)).map((n) => n.getText(ast)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
}
const appRenameJs = compileFunctions(await readFile(new URL('../src/App.vue', import.meta.url), 'utf8'), ['renameShortcut'])
function appRenameFixture() {
  let tabRenames = 0
  const context = vm.createContext({ matchShortcut, disposed: false,
    store: { ready: true, busy: false, settings: { shortcuts: {} }, projects: [{ id: 'p', name: 'Project' }] },
    renameTarget: { target: { kind: 'project', id: 'p' }, keydown() {} },
    document: { querySelectorAll: () => [] }, sidebarVisible: ref(true), sidebarView: ref('projects'),
    deck: { value: { renameActiveTab: () => { tabRenames++; return true } } },
  })
  for (const name of ['renamingProject', 'projectDialog', 'groupDialog', 'deleting', 'settingsDialog', 'closeDialog', 'worktreeDialog', 'recoveryDialog']) context[name] = ref(null)
  vm.runInContext(appRenameJs, context)
  const press = (extra = {}) => {
    const event = { ...key('F2'), key: 'F2', preventDefault() { this.prevented = true }, stopImmediatePropagation() { this.stopped = true }, ...extra }
    context.renameShortcut(event)
    return event
  }
  return { context, press, tabRenames: () => tabRenames }
}

test('App rename routes project and tab targets and consumes successful events', () => {
  const { context, press, tabRenames } = appRenameFixture()
  assert.equal(press().stopped, true)
  assert.equal(context.renamingProject.value.id, 'p')
  context.renamingProject.value = null
  context.renameTarget.target = { kind: 'workspace' }
  assert.equal(press().prevented, true)
  assert.equal(tabRenames(), 1)
  context.deck.value.renameActiveTab = () => false
  assert.equal(press().prevented, undefined)
  context.renameTarget.target = { kind: 'project', id: 'p' }
  context.document.querySelectorAll = () => [{ getClientRects: () => [] }]
  assert.equal(press().stopped, true)
})

test('Rename shortcuts preserve input defaults through both app handlers', async () => {
  for (const binding of [key('F2'), key('KeyA', { ctrlKey: true })]) {
    const { context, press, tabRenames } = appRenameFixture()
    const { tracker, element } = renameTrackerFixture()
    const input = element(['input'])
    tracker.focusin({ target: input })
    context.renameTarget = tracker
    context.store.settings.shortcuts = { rename: [binding] }
    vm.runInContext(handlerJs, context)
    const event = press({ ...binding, target: input, stopPropagation() { this.stopped = true } })
    await context.workspaceShortcut(event)
    assert.equal(event.prevented, undefined)
    assert.equal(event.stopped, undefined)
    assert.equal(context.renamingProject.value, null)
    assert.equal(tabRenames(), 0)
  }
})

test('App rename ignores blocked, repeated, composing, missing and hidden targets', () => {
  for (const blocked of ['renamingProject', 'projectDialog', 'groupDialog', 'deleting', 'settingsDialog', 'closeDialog', 'worktreeDialog', 'recoveryDialog']) {
    const { context, press } = appRenameFixture()
    context[blocked].value = true
    assert.equal(press().stopped, undefined, blocked)
  }
  for (const mutate of [
    (c) => { c.store.busy = true }, (c) => { c.store.ready = false }, (c) => { c.disposed = true },
    (c) => { c.document.querySelectorAll = () => [{ getClientRects: () => [1] }] }, (c) => { c.store.projects = [] },
    (c) => { c.renameTarget.target = null }, (c) => { c.sidebarVisible.value = false }, (c) => { c.sidebarView.value = 'files' },
  ]) {
    const { context, press } = appRenameFixture()
    mutate(context)
    assert.equal(press().stopped, undefined)
  }
  for (const extra of [{ repeat: true }, { isComposing: true }]) assert.equal(appRenameFixture().press(extra).stopped, undefined)
})

const renameActiveJs = compileFunctions(workspaceSource, ['renameActiveTab'])
test('Workspace rename uses the active tab and guards lifecycle, dialogs and editor operations', () => {
  for (const blocked of [null, 'disposed', 'inactive', 'unready', 'busy', 'shortcutsBlocked', 'confirmation', 'rename', 'quickOpen', 'tabMenu', 'editorDialog', 'empty']) {
    const context = vm.createContext({ disposed: false, props: { active: true, shortcutsBlocked: false, editors: { entries: new Map() } },
      ready: ref(true), busy: ref(false), activeTab: ref({ id: 'tab', title: 'Title' }), confirmation: ref(null), rename: ref(null), quickOpen: ref(false), tabMenu: ref(null) })
    if (blocked === 'disposed') context.disposed = true
    else if (blocked === 'inactive') context.props.active = false
    else if (blocked === 'unready') context.ready.value = false
    else if (blocked === 'shortcutsBlocked') context.props.shortcutsBlocked = true
    else if (blocked === 'editorDialog') context.props.editors.entries.set('editor', { dialog: {} })
    else if (blocked === 'empty') context.activeTab.value = null
    else if (blocked) context[blocked].value = true
    vm.runInContext(renameActiveJs, context)
    assert.equal(context.renameActiveTab(), blocked === null, String(blocked))
    if (!blocked) { assert.equal(context.rename.value.id, 'tab'); assert.equal(context.rename.value.title, 'Title') }
  }
})

const projectRenameJs = compileFunctions(await readFile(new URL('../src/components/dialogs/ProjectRenameDialog.vue', import.meta.url), 'utf8'), ['save', 'cancel'])
function projectRenameFixture() {
  const updates = []
  const current = { id: 'p', name: 'Old', path: 'C:/project', groupId: 'g', favorite: true, hidden: true }
  const context = vm.createContext({ project: ref(current), name: ref('  New  '), error: ref(''), saving: ref(false),
    store: { projects: [current], busy: false, ready: true, error: '', run: async (action) => { await action(); return true } },
    projectApi: { updateProject: async (...args) => { updates.push(args) } }, t: (key) => key })
  vm.runInContext(projectRenameJs, context)
  return { context, updates }
}

test('Project rename trims only the name, preserves other values and closes on success', async () => {
  const { context, updates } = projectRenameFixture()
  await context.save()
  assert.equal(updates.length, 1)
  assert.equal(updates[0][0], 'p')
  assert.deepEqual(JSON.parse(JSON.stringify(updates[0][1])), { id: 'p', name: 'New', path: 'C:/project', groupId: 'g', favorite: true, hidden: true })
  assert.equal(context.project.value, null)
  assert.equal(context.saving.value, false)
})

test('Project rename validates input, preserves failed edits and cancels without saving', async () => {
  for (const mutate of [(c) => { c.name.value = ' ' }, (c) => { c.name.value = 'x'.repeat(101) }, (c) => { c.store.projects = [] }, (c) => { c.store.busy = true }, (c) => { c.saving.value = true }, (c) => { c.store.ready = false }]) {
    const { context, updates } = projectRenameFixture()
    mutate(context)
    await context.save()
    assert.equal(updates.length, 0)
  }
  const { context, updates } = projectRenameFixture()
  context.store.run = async () => { context.store.error = 'Save failed'; return false }
  await context.save()
  assert.equal(context.error.value, 'Save failed')
  assert.equal(context.name.value, '  New  ')
  assert.equal(context.project.value.id, 'p')
  assert.equal(context.saving.value, false)
  context.cancel()
  assert.equal(context.project.value, null)
  assert.equal(updates.length, 0)
})

test('Project rename preloads the name and focuses and selects it after rendering', async (t) => {
  const source = await readFile(new URL('../src/components/dialogs/ProjectRenameDialog.vue', import.meta.url), 'utf8')
  const ast = ts.createSourceFile('ProjectRename.ts', source.split('<script setup lang="ts">')[1].split('</script>')[0], ts.ScriptTarget.Latest, true)
  const watcher = ast.statements.find((n) => ts.isExpressionStatement(n) && ts.isCallExpression(n.expression) && n.expression.expression.getText(ast) === 'watch')
  const calls = []
  const context = vm.createContext({ project: ref(null), name: ref(''), error: ref('old error'), nextTick,
    input: ref({ focus: () => calls.push('focus'), select: () => calls.push('select') }),
    watch: (...args) => { const stop = watch(...args); t.after(stop); return stop } })
  vm.runInContext(ts.transpileModule(watcher.getText(ast), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  context.project.value = { id: 'p', name: 'Existing' }
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(context.name.value, 'Existing')
  assert.equal(context.error.value, '')
  assert.deepEqual(calls, ['focus', 'select'])
})
