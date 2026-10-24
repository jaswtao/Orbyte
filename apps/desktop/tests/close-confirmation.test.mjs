/** 关闭确认行为。Author: jaswtao · Date: 2026-10-16 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'

const source = await readFile(new URL('../src/components/workspace/WorkspaceView.vue', import.meta.url), 'utf8')
const script = source.split('<script setup lang="ts">')[1].split('</script>')[0]
const ast = ts.createSourceFile('WorkspaceView.ts', script, ts.ScriptTarget.Latest, true)
const names = new Set(['confirmIfRunning', 'closePane', 'closeTab', 'stop', 'confirmAction', 'reconcile'])
const body = ast.statements.filter((node) => ts.isFunctionDeclaration(node) && names.has(node.name?.text)).map((node) => node.getText(ast)).join('\n')
const compiled = ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

function t(key, params = {}) {
  const messages = {
    'workspace.taskCheckFailed': 'Could not check running terminal tasks, so close confirmation is required: {reason}',
    'workspace.unsavedSuffix': '{text} {count} file(s) have unsaved changes.',
    'workspace.closeEditor': 'Close this editor?',
    'workspace.closePane': 'Closing this pane stops the terminal and its managed tasks. Continue?',
    'workspace.closeTab': 'Closing “{title}” stops every terminal in it and their managed tasks. Continue?',
    'workspace.stopTerminal': 'Stop this terminal and its managed tasks? The configuration and pane stay.',
    'workspace.saveIncomplete': 'A file is still unsaved. Resolve the save error or conflict first.',
  }
  return (messages[key] ?? key).replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? ''))
}

function fixture(enabled, state = 'running', dirty = false, runningTasks = false) {
  const closed = [], stopped = [], discarded = [], errors = []
  const entry = { status: { state }, client: { id: 'session', paused: false,
    pauseInput() { this.paused = true }, resumeInput() { this.paused = false },
    updateStatus(status) { entry.status = status },
  }, intentionalStop: false }
  const editor = { document: { dirty } }
  const props = {
    settings: { confirm_close_terminal: enabled },
    registry: { byConfig: new Map([['terminal', entry]]) },
    editors: { entries: new Map(dirty ? [['file', editor]] : []), async save() { editor.document.dirty = false }, async discard(id) { discarded.push(id) } },
  }
  const confirmation = { value: null }
  const busy = { value: false }
  const context = vm.createContext({
    props, confirmation, busy, disposed: false, confirmSaving: { value: false },
    leaves: (root) => root,
    closeIds: async (ids) => { closed.push(...ids) },
    perform: async (action) => { await action() },
    withTimeout: (promise) => promise,
    terminalApi: { async terminalsHaveRunningTasks() { if (runningTasks instanceof Error) throw runningTasks; return runningTasks }, async stopTerminal(id) { stopped.push(id) },
      async listTerminalSessions() { return [{ terminalId: 'session', status: { state: stopped.length ? 'exited' : state, exitCode: 0 } }] },
    },
    error: (message) => errors.push(message),
    t,
  })
  vm.runInContext(compiled, context)
  return { props, confirmation, busy, closed, stopped, discarded, errors, run: (code) => vm.runInContext(code, context) }
}

test('开关控制页签和窗格，启动中和运行中的终端都遵循设置', async () => {
  for (const enabled of [true, false]) for (const state of ['starting', 'running', 'exited']) {
    for (const action of ["closePane('terminal')", "closeTab({ title: '终端', root: [{ type: 'terminal', terminalConfigId: 'terminal' }] })"]) {
      const f = fixture(enabled, state)
      await f.run(action)
      const expected = (enabled || state === 'starting') && state !== 'exited'
      assert.equal(!!f.confirmation.value, expected)
      assert.deepEqual(f.closed, expected ? [] : ['terminal'])
    }
  }
})

test('设置即时生效，多分屏页签一次关闭所有终端', async () => {
  const f = fixture(true)
  f.props.settings.confirm_close_terminal = false
  await f.run("closeTab({ title: '分屏', root: [{ type: 'terminal', terminalConfigId: 'terminal' }, { type: 'terminal', terminalConfigId: 'other' }] })")
  assert.equal(f.confirmation.value, null)
  assert.deepEqual(f.closed, ['terminal', 'other'])
})

test('关闭开关仍保护未保存文件，取消、保存关闭和放弃修改保持可用', async () => {
  for (const saveFiles of [true, false]) {
    const f = fixture(false, 'running', true)
    await f.run("closeTab({ title: '混合', root: [{ type: 'terminal', terminalConfigId: 'terminal' }, { type: 'editor', id: 'file' }] })")
    assert.ok(f.confirmation.value)
    assert.equal(f.confirmation.value.dirtyIds[0], 'file')
    assert.deepEqual(f.closed, [])
    // 取消只清除待执行操作，不关闭任何窗格。
    f.confirmation.value = null
    assert.deepEqual(f.closed, [])
    await f.run("closePane('file')")
    await f.run(`confirmAction(${saveFiles})`)
    assert.deepEqual(f.closed, ['file'])
    assert.equal(f.props.editors.entries.get('file').document.dirty, !saveFiles)
    assert.deepEqual(f.discarded, saveFiles ? [] : ['file'])
    assert.deepEqual(f.errors, [])
  }
})

test('结束终端仍要求确认，忙碌时不重复关闭', async () => {
  const f = fixture(false)
  await f.run("stop('terminal')")
  assert.ok(f.confirmation.value)
  assert.deepEqual(f.stopped, [])
  await f.run('confirmAction()')
  await new Promise(setImmediate)
  assert.deepEqual(f.stopped, ['session'])
  assert.equal(f.props.registry.byConfig.get('terminal').client.paused, true)
  assert.equal(f.props.registry.byConfig.get('terminal').status.state, 'exited')
  f.busy.value = true
  await f.run("closePane('terminal')")
  assert.deepEqual(f.closed, [])
})

test('运行中的智能体或包装脚本强制确认，取消不关闭，确认后执行关闭', async () => {
  for (const enabled of [true, false]) {
    for (const action of ["closePane('terminal')", "closeTab({ title: '智能体', root: [{ type: 'terminal', terminalConfigId: 'terminal' }] })"]) {
      const f = fixture(enabled, 'running', false, true)
      await f.run(action)
      assert.ok(f.confirmation.value)
      assert.deepEqual(f.closed, [])
      f.confirmation.value = null
      assert.deepEqual(f.closed, [])
      await f.run(action)
      await f.run('confirmAction()')
      assert.deepEqual(f.closed, ['terminal'])
    }
  }
})

test('任务查询失败仍确认，不静默关闭终端', async () => {
  const f = fixture(false, 'running', false, new Error('查询失败'))
  await f.run("closePane('terminal')")
  assert.ok(f.confirmation.value)
  assert.deepEqual(f.closed, [])
  assert.match(f.errors[0], /Could not check running terminal tasks/)
})

test('等待任务查询期间产生的文件修改仍需确认，并支持保存后关闭', async () => {
  let finishCheck
  const checking = new Promise((resolve) => { finishCheck = resolve })
  const f = fixture(false, 'running', true, checking)
  const editor = f.props.editors.entries.get('file')
  editor.document.dirty = false
  const pending = f.run("closeTab({ title: '混合', root: [{ type: 'terminal', terminalConfigId: 'terminal' }, { type: 'editor', id: 'file' }] })")
  assert.equal(f.busy.value, true)
  editor.document.dirty = true
  finishCheck(false)
  await pending
  assert.deepEqual(f.closed, [])
  assert.deepEqual(Array.from(f.confirmation.value.dirtyIds), ['file'])
  await f.run('confirmAction(true)')
  assert.equal(editor.document.dirty, false)
  assert.deepEqual(f.closed, ['terminal', 'file'])
})
