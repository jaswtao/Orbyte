/** 终端输入顺序与失败反馈。Author: jaswtao · Date: 2026-10-13 */
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import vm from 'node:vm'
import xterm from '@xterm/xterm'
import { compile } from 'vue'
import ts from 'typescript'
import { loadService } from './load-service.mjs'

const source = await readFile(new URL('../src/services/terminal/inputQueue.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } })
const { InputQueue } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
const imeSource = await readFile(new URL('../src/services/terminal/ime.ts', import.meta.url), 'utf8')
const imeOutput = ts.transpileModule(imeSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText
const { bindTerminalIme } = await import(`data:text/javascript;base64,${Buffer.from(imeOutput).toString('base64')}`)
const flush = () => new Promise((resolve) => setImmediate(resolve))
const { bindTerminalSelection } = await loadService('terminal/selection')
const { bindTerminalActivation } = await loadService('terminal/activation')

test('窗口激活只关联实际交互的终端，切到其他控件后清空会话', async () => {
  const document = new EventTarget()
  const calls = []
  let release
  const first = new Promise((resolve) => { release = resolve })
  const unbind = bindTerminalActivation(document, async (id) => {
    calls.push(id)
    if (calls.length === 1) await first
  })
  const event = (type, id) => {
    const value = new Event(type)
    Object.defineProperty(value, 'target', { value: { closest: () => id ? { dataset: { terminalSession: id } } : null } })
    document.dispatchEvent(value)
  }
  event('focusin', 'first')
  event('pointerdown', 'second')
  event('focusin', null)
  await flush()
  assert.deepEqual(calls, ['first'])
  release()
  await flush()
  assert.deepEqual(calls, ['first', 'second', null])
  event('terminal-session-attached', 'third')
  await flush()
  assert.equal(calls.at(-1), 'third')
  // 原生窗口失焦不清空会话，让连续比较保持同一个会话关联。
  event('blur', null)
  await flush()
  assert.equal(calls.at(-1), 'third')
  unbind()
  event('focusin', 'first')
  await flush()
  assert.equal(calls.at(-1), null)
})

test('键盘切换项目隐藏当前终端时清空激活会话，隐藏其他终端不影响当前会话', async () => {
  const document = new EventTarget()
  const calls = []
  const unbind = bindTerminalActivation(document, async (id) => { calls.push(id) })
  const event = (type, id) => {
    const value = new Event(type)
    Object.defineProperty(value, 'target', { value: { closest: () => ({ dataset: { terminalSession: id } }) } })
    document.dispatchEvent(value)
  }
  event('focusin', 'first')
  // v-show 隐藏终端后没有新的 focusin 或 pointerdown。
  event('terminal-session-hidden', 'first')
  await flush()
  assert.deepEqual(calls, ['first', null])
  event('focusin', 'second')
  event('terminal-session-hidden', 'first')
  await flush()
  assert.deepEqual(calls, ['first', null, 'second'])
  event('blur', 'second')
  await flush()
  assert.equal(calls.at(-1), 'second')
  unbind()
  event('terminal-session-hidden', 'second')
  await flush()
  assert.deepEqual(calls, ['first', null, 'second', null])
})

function selectionFixture(t, cols = 20) {
  const term = new xterm.Terminal({ cols, rows: 10 })
  const document = new EventTarget()
  const screen = new EventTarget()
  let range
  const calls = []
  const model = { selectionStartLength: 0 }
  const service = { _model: model, _selectLineAt() {
    model.selectionStartLength = (range.end.y + 1) * cols
  } }
  const original = service._selectLineAt
  const binding = bindTerminalSelection({
    element: { ownerDocument: document, querySelector: () => screen }, buffer: term.buffer, cols,
    get modes() { return term.modes },
    select: (...args) => calls.push(args), clearSelection: () => calls.push('clear'), focus() {},
    _core: { _selectionService: service },
    getSelectionPosition: () => range,
  })
  t.after(() => { binding.dispose(); term.dispose() })
  const event = (type) => document.dispatchEvent(new Event(type))
  return { term, calls, binding, model, service, original, event,
    down(overrides = {}) {
      const value = Object.assign(new Event('mousedown', { cancelable: true }), {
        button: 0, detail: 4, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...overrides,
      })
      screen.dispatchEvent(value)
      return value
    },
    async write(text) {
      term.reset()
      await new Promise((resolve) => term.write(text, resolve))
      range = { start: { x: 0, y: 0 }, end: { x: cols, y: term.buffer.active.cursorY } }
    },
    triple() {
      service._selectLineAt(0)
      calls.push(model.selectionStartLength ? [0, 0, model.selectionStartLength] : 'clear')
      event('mouseup')
    },
  }
}

test('三击剔除行尾空白，保留缩进、折行及宽字符', async (t) => {
  const f = selectionFixture(t)
  for (const [text, expected] of [
    ['PS C:\\Orbyte> ', [0, 0, 13]],
    ['  project   ', [0, 0, 9]],
    ['中文😀  ', [0, 0, 5]],
    ['e\u0301  ', [0, 0, 1]],
    ['x'.repeat(20), [0, 0, 20]],
    ['x'.repeat(23) + '   ', [0, 0, 23]],
    ['x'.repeat(19) + '中文😀   ', [0, 0, 25]],
    ['x'.repeat(20) + ' '.repeat(24), [0, 0, 20]],
    ['    ', 'clear'],
  ]) {
    await f.write(text)
    f.calls.length = 0
    f.triple()
    assert.deepEqual(f.calls, [expected], JSON.stringify(text))
  }
})

test('四击选择完整活动缓冲区，保留历史、内部空行与缩进并去掉末尾空白', async (t) => {
  const f = selectionFixture(t)
  for (const [text, expected] of [
    ['  first\r\n\r\n  last   \r\n\r\n', [0, 0, 46]],
    ['line\r\n'.repeat(15) + '中文😀  \r\n', [0, 0, 305]],
    ['x'.repeat(20), [0, 0, 20]],
    ['x'.repeat(23) + '    \r\n', [0, 0, 23]],
    ['e\u0301  ', [0, 0, 1]],
    ['  \r\n\r\n', 'clear'],
  ]) {
    await f.write(text)
    f.calls.length = 0
    assert.equal(f.down().defaultPrevented, true)
    assert.deepEqual(f.calls, [expected], JSON.stringify(text))
  }
  await f.write('normal screen')
  await new Promise((resolve) => f.term.write('\x1b[?1049h\x1b[Halt', resolve))
  f.calls.length = 0
  f.down()
  assert.deepEqual(f.calls, [[0, 0, 3]])
})

test('四击仅响应无修饰键左键，遵循鼠标上报并在销毁时移除监听', async (t) => {
  const f = selectionFixture(t)
  await f.write('project')
  for (const overrides of [
    { detail: 1 }, { detail: 2 }, { detail: 3 }, { detail: 5 }, { button: 2 },
    { altKey: true }, { ctrlKey: true }, { metaKey: true }, { shiftKey: true },
  ]) assert.equal(f.down(overrides).defaultPrevented, false)
  await new Promise((resolve) => f.term.write('\x1b[?1000h', resolve))
  assert.equal(f.down().defaultPrevented, false)
  assert.deepEqual(f.calls, [])
  await new Promise((resolve) => f.term.write('\x1b[?1000l', resolve))
  f.binding.dispose()
  assert.equal(f.down().defaultPrevented, false)
  assert.deepEqual(f.calls, [])
})

test('三击按下时已收紧选区，拖动恢复原生锚点，松开与销毁释放适配', async (t) => {
  const f = selectionFixture(t)
  await f.write('project  ')
  f.service._selectLineAt(0)
  assert.equal(f.model.selectionStartLength, 7)
  f.event('mousemove')
  assert.equal(f.model.selectionStartLength, 20)
  f.service._selectLineAt(0)
  f.event('mouseup')
  f.event('mousemove')
  assert.equal(f.model.selectionStartLength, 7)
  f.service._selectLineAt(0)
  f.binding.dispose()
  assert.equal(f.service._selectLineAt, f.original)
  f.event('mousemove')
  assert.equal(f.model.selectionStartLength, 7)
  f.service._selectLineAt(0)
  assert.equal(f.model.selectionStartLength, 20)
})

// 使用实际终端配置、缓冲和 xterm 选词算法，避免仅验证分隔符字符串。
test('双击路径按单词选择，保留点号和下划线的原有行为', async (t) => {
  const clientSource = await readFile(new URL('../src/services/terminal/terminalClient.ts', import.meta.url), 'utf8')
  let options
  const captured = new Error('已获取终端配置')
  const { TerminalClient } = loadCommonJs(clientSource, (name) => {
    if (name === '@xterm/xterm') return { Terminal: class {
      constructor(value) { options = value; throw captured }
    } }
    // 构造函数创建 xterm 之前只初始化这些依赖，不涉及 IPC 或 DOM。
    return new Proxy({}, { get: () => class {} })
  })
  assert.throws(() => new TerminalClient({}, {}), (error) => error === captured)
  const term = new xterm.Terminal({ ...options, cols: 120, rows: 5 })
  t.after(() => term.dispose())
  const selectionSource = await readFile(new URL('../node_modules/@xterm/xterm/src/browser/services/SelectionService.ts', import.meta.url), 'utf8')
  const { SelectionService } = loadCommonJs(selectionSource, (name) => name.endsWith('/lifecycle')
    ? { Disposable: class {} }
    : new Proxy({}, { get: () => () => {} }))
  const selection = Object.assign(Object.create(SelectionService.prototype), {
    _bufferService: term._core._bufferService,
    _optionsService: term._core.optionsService,
    _workCell: term.buffer.active.getNullCell(),
    _model: {},
  })
  const cases = [
    ['PS C:\\demo\\project\\sample-project\\Orbyte> project', 'project', 0],
    ['PS C:\\demo\\project\\sample-project\\Orbyte> project', 'project', 1],
    ['PS C:\\demo\\project\\sample-project\\Orbyte> project', 'Orbyte', 0],
    ['PS C:\\demo\\project\\sample-project\\Orbyte> project', 'project', 2],
    ['/data/personal-project/Orbyte', 'project', 0],
    ['git status', 'status', 0],
    ['file_name.txt', 'file_name.txt', 0],
    ['(project),"Orbyte"', 'project', 0],
  ]
  for (const [line, expected, occurrence] of cases) {
    term.reset()
    await new Promise((resolve) => term.write(line, resolve))
    let start = -1
    for (let index = 0; index <= occurrence; index++) start = line.indexOf(expected, start + 1)
    // 检查词内每个位置，确保边缘字符的双击也不会扩大选区。
    for (let offset = 0; offset < expected.length; offset++) {
      selection._selectWordAt([start + offset, 0], true)
      const [column] = selection._model.selectionStart
      assert.equal(line.slice(column, column + selection._model.selectionStartLength), expected, `${line} @ ${start + offset}`)
    }
  }
})

test('快速粘贴时只有一个写入在途，后续输入合并且顺序不变', async () => {
  const queue = new InputQueue()
  const sent = []
  const complete = []
  queue.push('首行\r\n')
  queue.setSender((data) => {
    sent.push(data)
    return new Promise((resolve) => complete.push(resolve))
  })
  queue.push('第二行\r\n')
  queue.push('第三行\r\n')
  assert.deepEqual(sent, ['首行\r\n'])
  complete.shift()()
  await flush()
  assert.deepEqual(sent, ['首行\r\n', '第二行\r\n第三行\r\n'])
  complete.shift()()
  await flush()
})

test('切换发送方时丢弃旧会话待发送输入', async () => {
  const queue = new InputQueue()
  const sent = []
  let complete
  queue.setSender((data) => {
    sent.push(`old:${data}`)
    return new Promise((resolve) => { complete = resolve })
  })
  queue.push('正在发送')
  queue.push('旧的待发送文本')
  queue.setSender(null)
  queue.clear()
  queue.setSender(async (data) => { sent.push(`new:${data}`) })
  queue.push('新的输入')
  complete()
  await flush()
  assert.deepEqual(sent, ['old:正在发送', 'new:新的输入'])
})

test('输入发送失败时报告错误，并恢复后续发送', async () => {
  const errors = []
  const queue = new InputQueue((error) => errors.push(error))
  const error = new Error('终端已结束')
  queue.setSender(async () => { throw error })
  queue.push('失败的输入')
  await flush()
  assert.deepEqual(errors, [error])
  const sent = []
  queue.setSender(async (data) => { sent.push(data) })
  queue.push('重启后的输入')
  await flush()
  assert.deepEqual(sent, ['重启后的输入'])
})

// 使用实际 xterm 按键处理与输入法处理代码，复现 keyCode=229 时的输入丢失。
function loadCommonJs(source, require, globals = {}) {
  const { outputText } = ts.transpileModule(source, { compilerOptions: {
    target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, experimentalDecorators: true,
  } })
  const exports = {}
  vm.runInNewContext(outputText, { exports, require, setTimeout, ...globals })
  return exports
}
const compositionSource = await readFile(new URL('../node_modules/@xterm/xterm/src/browser/input/CompositionHelper.ts', import.meta.url), 'utf8')
const { CompositionHelper } = loadCommonJs(compositionSource, (name) => name.endsWith('EscapeSequences')
  ? { C0: { ESC: '\x1b' } }
  : new Proxy({}, { get: () => () => {} }))
const registrySource = await readFile(new URL('../src/services/terminal/registry.ts', import.meta.url), 'utf8')
const shortcuts = await loadService('workspace/shortcuts')
const appearance = await loadService('appearance')
const recovery = await loadService('terminal/recovery')
const clipboard = { read: async () => [], readText: async () => '', writeText: async () => {}, nativePaste: async () => false }
// Node 的 EventTarget 没有 DOM 父子传播；分开调度捕获和冒泡以覆盖宿主拦截。
class TerminalHost extends EventTarget {
  capture = new EventTarget()
  addEventListener(type, listener, options) {
    if (options === true || options?.capture) this.capture.addEventListener(type, listener)
    else super.addEventListener(type, listener, options)
  }
  dispatchEvent(event) {
    this.capture.dispatchEvent(event)
    return event.cancelBubble ? !event.defaultPrevented : super.dispatchEvent(event)
  }
  dispatchFrom(target, document, event) {
    this.parentCapture?.dispatchEvent(event)
    this.capture.dispatchEvent(event)
    if (!event.cancelBubble) target.dispatchEvent(event)
    if (!event.cancelBubble) super.dispatchEvent(event)
    if (!event.cancelBubble) this.parentBubble?.dispatchEvent(event)
    if (!event.cancelBubble) document.dispatchEvent(event)
    return event
  }
}
function translate(key, params = {}) {
  const messages = {
    'terminal.pasteFailed': 'Paste failed: {reason}',
    'terminal.copyFailed': 'Copy failed: {reason}',
    'terminal.outputGap': 'Output gap: discarded {bytes} bytes',
    'terminal.screenshotTooLarge': 'A screenshot cannot exceed 50 MiB',
  }
  return (messages[key] ?? key).replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? ''))
}

const { TerminalRegistry } = loadCommonJs(registrySource, (name) => {
  if (name === '../appearance') return appearance
  if (name === './recovery') return recovery
  if (name === '../terminalApi') return { terminalApi: {
    readClipboardText: () => clipboard.readText(),
    writeClipboardText: (text) => clipboard.writeText(text),
    saveClipboardImage: () => clipboard.read(),
    terminalUsesNativePaste: (id) => clipboard.nativePaste(id),
  } }
  if (name === 'vue') return { markRaw: (value) => value, shallowReactive: (value) => value }
  if (name === '../workspace/shortcuts') return shortcuts
  if (name === '../../i18n/translate') return { t: translate }
  if (name === './terminalClient') return { TerminalClient: class {
    term = new xterm.Terminal()
  } }
  throw new Error(`未预期的依赖：${name}`)
}, { document: { querySelector: () => null, createElement: () => new TerminalHost() }, navigator: { clipboard: {
  readText: async () => { throw new DOMException('Read permission denied', 'NotAllowedError') },
  writeText: async () => { throw new DOMException('Write permission denied', 'NotAllowedError') },
  read: async () => { throw new DOMException('Read permission denied', 'NotAllowedError') },
} } })

function keyboardFixture(custom = true, readText = async () => '', errors = [], settings = { shortcuts: {} }, blocked = false) {
  clipboard.readText = readText
  clipboard.read = async () => null
  clipboard.writeText = async () => {}
  clipboard.nativePaste = async () => false
  const entry = custom ? new TerminalRegistry(() => 'dark', () => settings, () => blocked).create({}, (message) => errors.push(message)) : null
  const term = entry ? entry.client.term : new xterm.Terminal()
  const core = term._core
  core.textarea = { value: '', focus() {} }
  core._compositionHelper = new CompositionHelper(core.textarea, {}, {}, {}, core.coreService, {})
  core.updateCursorStyle = () => {}
  core._showCursor = () => {}
  const sent = []
  term.onData((data) => sent.push(data))
  return { term, sent, client: entry?.client, host: entry?.host, key: (overrides = {}) => {
    const event = { type: 'keydown', key: 'Process', code: 'Escape', keyCode: 229,
      ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, isComposing: false,
      preventDefault() { this.defaultPrevented = true },
      stopPropagation() { this.propagationStopped = true },
      getModifierState: () => false, ...overrides }
    core._keyDown(event)
    return event
  } }
}

test('输入法下 Esc 在原生 xterm 丢失，兼容处理只发送一个 ESC 并阻止冒泡', async () => {
  const original = keyboardFixture(false)
  original.key()
  await flush()
  assert.deepEqual(original.sent, [])
  original.term.dispose()
  const fixed = keyboardFixture()
  const event = fixed.key()
  fixed.term._core._keyUp({ ...event, type: 'keyup' })
  await flush()
  assert.deepEqual(fixed.sent, ['\x1b'])
  assert.equal(event.defaultPrevented, true)
  assert.equal(event.propagationStopped, true)
  fixed.term.dispose()
})

test('普通 Esc 正常传递，组词、可打印输入和应用快捷键保持原有处理', async () => {
  const fixture = keyboardFixture()
  fixture.key({ key: 'Escape', keyCode: 27 })
  fixture.key({ isComposing: true })
  fixture.key({ code: 'KeyA' })
  fixture.key({ key: 't', code: 'KeyT', keyCode: 84, ctrlKey: true, shiftKey: true })
  await flush()
  assert.deepEqual(fixture.sent, ['\x1b'])
  fixture.term.dispose()
})

test('Ctrl+V 与 Ctrl+Shift+V 均粘贴文本，保留多行和括号粘贴且不发送图片快捷键', async () => {
  for (const shiftKey of [false, true]) {
    let reads = 0
    const fixture = keyboardFixture(true, async () => { reads++; return '第一行\r\n第二行\n' })
    fixture.term._core.coreService.decPrivateModes.bracketedPasteMode = true
    const event = fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true, shiftKey })
    fixture.term._core._keyUp({ ...event, type: 'keyup' })
    await flush()
    assert.deepEqual(fixture.sent, ['\x1b[200~第一行\r第二行\r\x1b[201~'])
    assert.equal(reads, 1)
    assert.equal(event.defaultPrevented, true)
    fixture.term.dispose()
  }
})

test('Windows Codex pastes Chinese and multiline clipboard text with one native shortcut', async () => {
  for (const text of ['是否本年就业应届毕业生', '是否本年就业应届毕\n业生\n', '中文😀\t“引号”'.repeat(1000)]) {
    for (const method of ['ctrl-v', 'ctrl-shift-v', 'contextmenu']) {
      const fixture = keyboardFixture(true, async () => text)
      fixture.client.id = 'codex-session'
      clipboard.nativePaste = async (id) => { assert.equal(id, 'codex-session'); return true }
      fixture.term._core.coreService.decPrivateModes.bracketedPasteMode = true
      if (method === 'contextmenu') fixture.host.dispatchEvent(new Event('contextmenu', { cancelable: true }))
      else fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true, shiftKey: method === 'ctrl-shift-v' })
      await flush()
      assert.deepEqual(fixture.sent, ['\x16'])
      fixture.term.dispose()
    }
  }
})

test('non-Codex terminals retain bracketed paste and a replaced session receives no stale paste', async () => {
  const fixture = keyboardFixture(true, async () => '是否本年就业应届毕\n业生')
  fixture.client.id = 'shell-session'
  fixture.term._core.coreService.decPrivateModes.bracketedPasteMode = true
  fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true })
  await flush()
  assert.deepEqual(fixture.sent, ['\x1b[200~是否本年就业应届毕\r业生\x1b[201~'])
  fixture.sent.length = 0
  let finish
  clipboard.nativePaste = () => new Promise((resolve) => { finish = resolve })
  fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true })
  await flush()
  fixture.client.id = 'replacement-session'
  finish(true)
  await flush()
  assert.deepEqual(fixture.sent, [])
  fixture.term.dispose()
})

test('剪贴板无文本和图片时不发送控制字符，Ctrl+Shift+V 只粘贴文本', async () => {
  const fixture = keyboardFixture()
  for (const shiftKey of [false, true]) {
    fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true, shiftKey })
    await flush()
  }
  assert.deepEqual(fixture.sent, [])
  fixture.term.dispose()
})

test('Ctrl+V 读取剪贴板失败时提示错误，不发送控制字符', async () => {
  const errors = []
  const fixture = keyboardFixture(true, async () => { throw new Error('剪贴板不可用') }, errors)
  fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true })
  await flush()
  assert.deepEqual(fixture.sent, [])
  assert.deepEqual(errors, ['Paste failed: Error: 剪贴板不可用'])
  fixture.term.dispose()
})

test('WebView 拒绝剪贴板权限时通过原生命令粘贴截图，文本快捷键不读取图片', async () => {
  const errors = []
  const fixture = keyboardFixture(true, async () => null, errors)
  let images = 0
  clipboard.read = async () => { images++; return 'C:\\cache\\screenshot.png' }
  fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true, shiftKey: true })
  await flush()
  assert.equal(images, 0)
  fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true })
  await flush()
  assert.equal(images, 1)
  assert.deepEqual(fixture.sent, ['"C:/cache/screenshot.png" '])
  assert.deepEqual(errors, [])
  fixture.term.dispose()
})

test('原生图片读取失败仍报告错误且不发送终端输入', async () => {
  const errors = []
  const fixture = keyboardFixture(true, async () => null, errors)
  clipboard.read = async () => { throw new Error('Clipboard occupied') }
  fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true })
  await flush()
  assert.deepEqual(fixture.sent, [])
  assert.deepEqual(errors, ['Paste failed: Error: Clipboard occupied'])
  fixture.term.dispose()
})

test('选中文本后首次右键只复制、清除选区并恢复焦点，再次右键才粘贴', async () => {
  let reads = 0
  const errors = []
  const fixture = keyboardFixture(true, async () => { reads++; throw new Error('剪贴板不可用') }, errors)
  const copied = []
  clipboard.writeText = async (text) => { copied.push(text) }
  let selected = true
  let focused = false
  fixture.term.hasSelection = () => selected
  fixture.term.getSelection = () => '选中的文本\r\n第二行'
  fixture.term.clearSelection = () => { selected = false }
  fixture.term.focus = () => { focused = true }
  fixture.term._core.coreService.decPrivateModes.bracketedPasteMode = true
  const event = new Event('contextmenu', { cancelable: true })
  fixture.host.dispatchEvent(event)
  await flush()
  assert.deepEqual(fixture.sent, [])
  assert.equal(selected, false)
  assert.equal(focused, true)
  assert.equal(event.defaultPrevented, true)
  assert.equal(reads, 0)
  assert.deepEqual(copied, ['选中的文本\r\n第二行'])
  assert.deepEqual(errors, [])
  clipboard.readText = async () => { reads++; return copied.at(-1) }
  focused = false
  fixture.host.dispatchEvent(new Event('contextmenu', { cancelable: true }))
  await flush()
  assert.deepEqual(fixture.sent, ['\x1b[200~选中的文本\r第二行\x1b[201~'])
  assert.equal(reads, 1)
  assert.equal(copied.length, 1)
  assert.equal(focused, true)
  fixture.term.dispose()
})

test('右键复制到剪贴板失败时提示错误、清除选区并恢复焦点，不发送终端输入', async () => {
  const errors = []
  const fixture = keyboardFixture(true, async () => '', errors)
  clipboard.writeText = async () => { throw new Error('剪贴板不可用') }
  let selected = true
  let focused = false
  fixture.term.hasSelection = () => selected
  fixture.term.getSelection = () => '选中的文本'
  fixture.term.clearSelection = () => { selected = false }
  fixture.term.focus = () => { focused = true }
  fixture.host.dispatchEvent(new Event('contextmenu', { cancelable: true }))
  await flush()
  assert.deepEqual(fixture.sent, [])
  assert.equal(selected, false)
  assert.equal(focused, true)
  assert.deepEqual(errors, ['Copy failed: Error: 剪贴板不可用'])
  fixture.term.dispose()
})

test('无选区时右键粘贴剪贴板文本并恢复焦点', async () => {
  const fixture = keyboardFixture(true, async () => '剪贴板文本')
  let focused = false
  fixture.term.focus = () => { focused = true }
  fixture.host.dispatchEvent(new Event('contextmenu', { cancelable: true }))
  await flush()
  assert.deepEqual(fixture.sent, ['剪贴板文本'])
  assert.equal(focused, true)
  fixture.term.dispose()
})

function mouseFixture(t, readText, errors = []) {
  const fixture = keyboardFixture(true, readText, errors)
  const core = fixture.term._core
  const element = new EventTarget()
  const document = new EventTarget()
  element.classList = { add() {}, remove() {} }
  core.element = element
  core._document = document
  core._mouseService = { getMouseReportCoords: () => ({ col: 0, row: 0, x: 0, y: 0 }) }
  core._selectionService = { shouldForceSelection: () => false, disable() {}, enable() {} }
  core.focus = () => {}
  core.bindMouse()
  core.coreMouseService.activeProtocol = 'VT200'
  core.coreMouseService.activeEncoding = 'SGR'
  t.after(() => fixture.term.dispose())
  return { ...fixture, mouse(type, button = 2) {
    const event = Object.assign(new Event(type, { cancelable: true, bubbles: true }), {
      button, buttons: type === 'mousedown' ? [1, 4, 2][button] : 0,
      ctrlKey: false, altKey: false, shiftKey: false, deltaY: -1,
    })
    return fixture.host.dispatchFrom(element, document, event)
  }, rightClick() {
    this.mouse('mousedown')
    this.mouse('mouseup')
    return this.mouse('contextmenu')
  } }
}

test('开启鼠标上报后完整右键只粘贴一次，连续右键不丢失粘贴', async (t) => {
  let reads = 0
  const fixture = mouseFixture(t, async () => { reads++; return '第一行\r\n第二行\n' })
  fixture.term._core.coreService.decPrivateModes.bracketedPasteMode = true
  for (let count = 1; count <= 2; count++) {
    assert.equal(fixture.rightClick().defaultPrevented, true)
    await flush()
    assert.equal(reads, count)
    assert.deepEqual(fixture.sent, Array(count).fill('\x1b[200~第一行\r第二行\r\x1b[201~'))
  }
})

test('开启鼠标上报后右键只复制选区，读取失败不发送鼠标或文本输入', async (t) => {
  let reads = 0
  const errors = []
  const fixture = mouseFixture(t, async () => { reads++; throw new Error('剪贴板不可用') }, errors)
  const copied = []
  clipboard.writeText = async (text) => { copied.push(text) }
  let selected = true
  fixture.term.hasSelection = () => selected
  fixture.term.getSelection = () => '选中的文本'
  fixture.term.clearSelection = () => { selected = false }
  fixture.rightClick()
  await flush()
  assert.deepEqual(copied, ['选中的文本'])
  assert.equal(selected, false)
  assert.equal(reads, 0)
  assert.deepEqual(fixture.sent, [])
  fixture.rightClick()
  await flush()
  assert.equal(reads, 1)
  assert.deepEqual(fixture.sent, [])
  assert.deepEqual(errors, ['Paste failed: Error: 剪贴板不可用'])
})

test('右键拦截保留左键、中键与滚轮的实际 xterm 鼠标上报', (t) => {
  const fixture = mouseFixture(t, async () => '')
  for (const button of [0, 1]) {
    fixture.mouse('mousedown', button)
    fixture.mouse('mouseup', button)
  }
  fixture.term._core.coreMouseService.consumeWheelEvent = () => -1
  fixture.mouse('wheel', 0)
  assert.deepEqual(fixture.sent, [
    '\x1b[<0;1;1M', '\x1b[<0;1;1m',
    '\x1b[<1;1;1M', '\x1b[<1;1;1m', '\x1b[<64;1;1M',
  ])
})

test('右键其他终端时先同步窗格焦点，关闭操作针对该窗格且只粘贴一次', async (t) => {
  const source = await readFile(new URL('../src/components/workspace/LayoutPane.vue', import.meta.url), 'utf8')
  const render = compile(source.match(/<template>([\s\S]*)<\/template>/)[1])
  const { removeFromLayout, leaves } = await loadService('workspace/layout')
  const fixture = mouseFixture(t, async () => '粘贴内容')
  let focused = 'first'
  // 使用真实模板编译出的监听配置，模拟父窗格到终端宿主的传播顺序。
  const pane = render({
    node: { type: 'terminal', terminalConfigId: 'second' }, focused,
    config: null, entry: null, statusText: '', hint: () => '',
    focus: () => { focused = 'second' },
    directoryLabel: '',
    t: translate,
  }, [])
  fixture.host.parentCapture = new EventTarget()
  fixture.host.parentBubble = new EventTarget()
  for (const [key, handler] of Object.entries(pane.props)) {
    if (!key.startsWith('on')) continue
    const capture = key.endsWith('Capture')
    const type = key.slice(2, capture ? -7 : undefined).toLowerCase()
    fixture.host[capture ? 'parentCapture' : 'parentBubble'].addEventListener(type, handler)
  }
  fixture.rightClick()
  await flush()
  assert.equal(focused, 'second')
  assert.deepEqual(fixture.sent, ['粘贴内容'])
  const remaining = removeFromLayout({ schemaVersion: 1, activeTabId: 'a', tabs: [{
    id: 'a', title: 'A', root: { type: 'split', direction: 'horizontal', ratio: 0.5,
      children: ['first', 'second'].map((terminalConfigId) => ({ type: 'terminal', terminalConfigId })) },
  }] }, focused)
  assert.deepEqual(leaves(remaining.tabs[0].root).map((leaf) => leaf.terminalConfigId), ['first'])
})

function imeFixture(terminal) {
  const textarea = new EventTarget()
  textarea.value = ''
  textarea.style = {}
  const composition = { style: {}, classList: { add() {}, remove() {} }, getBoundingClientRect: () => ({ width: 2000, height: 20 }) }
  const screen = { clientWidth: 800, clientHeight: 480 }
  const buffer = { cursorX: 79, cursorY: 5, baseY: 10, viewportY: 10, isCursorInViewport: true, getLine: () => ({ length: 0, getCell: () => undefined }) }
  Object.defineProperties(buffer, { x: { get: () => buffer.cursorX }, y: { get: () => buffer.cursorY } })
  const listeners = new Map()
  const term = { textarea, element: { querySelector: (selector) => selector === '.xterm-screen' ? screen : composition },
    cols: 80, rows: 24, buffer: terminal?.buffer ?? { active: buffer }, options: { fontFamily: 'monospace', fontSize: 16 } }
  term.parser = terminal?.parser ?? {
    registerCsiHandler({ prefix = '', intermediates = '', final }, callback) {
      const key = `${prefix}${intermediates}${final}`
      listeners.set(key, callback)
      return { dispose: () => listeners.delete(key) }
    },
    registerEscHandler({ final }, callback) {
      listeners.set(final, callback)
      return { dispose: () => listeners.delete(final) }
    },
  }
  for (const name of ['onRender', 'onCursorMove', 'onResize', 'onScroll']) {
    term[name] = (callback) => { listeners.set(name, callback); return { dispose: () => listeners.delete(name) } }
  }
  const helper = new CompositionHelper(textarea, composition, { buffer, cols: term.cols }, { rawOptions: term.options }, {},
    { dimensions: { css: { cell: { width: 10, height: 20 } } } })
  // xterm 的监听先注册，兼容逻辑必须在其更新尺寸后约束输入框。
  textarea.addEventListener('compositionstart', () => helper.compositionstart())
  textarea.addEventListener('compositionupdate', () => helper.compositionupdate({ data: 'chang'.repeat(40) }))
  return { term, buffer, screen, textarea, composition, listeners, binding: bindTerminalIme(term) }
}

test('开始组词时立即校准锚点，长拼音和 xterm 延迟更新均不越过终端右边界', async () => {
  const fixture = imeFixture()
  fixture.textarea.style.left = '1500px'
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(fixture.textarea.style.left, '790px')
  assert.equal(fixture.textarea.style.top, '100px')
  fixture.textarea.dispatchEvent(new Event('compositionupdate'))
  await new Promise((resolve) => setTimeout(resolve, 10))
  // xterm 会把 width 扩到 2000px，随后的校准把单格 width 和 maxWidth 一并写回。
  assert.equal(fixture.textarea.style.left, '790px')
  assert.equal(fixture.textarea.style.width, '10px')
  assert.equal(fixture.textarea.style.maxWidth, '10px')
  assert.equal(fixture.composition.style.maxWidth, '10px')
  assert.equal(fixture.composition.style.overflow, 'hidden')
  fixture.binding.dispose()
})

function inverseLine(columns) {
  const inverse = new Set(columns)
  return { length: 80, getCell: (x) => x >= 0 && x < 80 ? {
    isInverse: () => inverse.has(x) ? 1 : 0, getWidth: () => 1, isFgRGB: () => false,
  } : undefined }
}

test('孤立反色格覆盖硬件光标，延迟覆写和短暂消失后仍锚在假光标', async () => {
  const fixture = imeFixture()
  fixture.listeners.get('?l')([25])
  // viewportY 是 10，绝对行 12 即视口第 2 行；硬件光标仍在第 79 列。
  fixture.buffer.getLine = (y) => y === 12 ? inverseLine([4]) : { length: 0, getCell: () => undefined }
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(fixture.textarea.style.left, '40px')
  assert.equal(fixture.textarea.style.top, '40px')
  assert.equal(fixture.composition.style.left, '40px')
  fixture.textarea.dispatchEvent(new Event('compositionupdate'))
  await new Promise((resolve) => setTimeout(resolve, 10))
  assert.equal(fixture.textarea.style.left, '40px')
  assert.equal(fixture.textarea.style.top, '40px')
  assert.equal(fixture.composition.style.left, '40px')
  fixture.buffer.getLine = () => ({ length: 0, getCell: () => undefined })
  fixture.listeners.get('onRender')()
  assert.equal(fixture.textarea.style.left, '40px')
  fixture.textarea.dispatchEvent(new Event('compositionend'))
  assert.equal(fixture.textarea.style.left, '790px')
  fixture.binding.dispose()
})

test('成片反色不当作假光标，仍跟随硬件光标', () => {
  const fixture = imeFixture()
  fixture.listeners.get('?l')([25])
  fixture.buffer.getLine = (y) => y === 12 ? inverseLine([4, 5, 6]) : { length: 0, getCell: () => undefined }
  fixture.listeners.get('onRender')()
  assert.equal(fixture.textarea.style.left, '790px')
  assert.equal(fixture.textarea.style.top, '100px')
  fixture.binding.dispose()
})

test('真实 xterm 缓冲中的普通反色输出不覆盖 Shell 输入位置', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  await new Promise((resolve) => terminal.write('\x1b[7mX\x1b[0m\r\nPS> ', resolve))
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(fixture.textarea.style.left, '40px')
  assert.equal(fixture.textarea.style.top, '20px')
})

test('真实 xterm 宽字符假光标按整个字符识别，显示硬件光标后立即解除锚定', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  // 普通缓冲区中的 TUI 也能隐藏硬件光标并绘制假光标。
  await new Promise((resolve) => terminal.write('\x1b[?25l\x1b[3;5H\x1b[7m中\x1b[0m\x1b[24;1H', resolve))
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(fixture.textarea.style.left, '40px')
  assert.equal(fixture.textarea.style.top, '40px')
  assert.equal(fixture.composition.style.left, '40px')
  await new Promise((resolve) => terminal.write('\x1b[?25h', resolve))
  fixture.listeners.get('onRender')()
  assert.equal(fixture.textarea.style.left, '0px')
  assert.equal(fixture.textarea.style.top, '460px')
})

test('两个相邻的反色窄字符不是一个宽字符假光标', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  await new Promise((resolve) => terminal.write('\x1b[?25l\x1b[3;5H\x1b[7mAB\x1b[0m\x1b[24;1H', resolve))
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(fixture.textarea.style.left, '0px')
  assert.equal(fixture.textarea.style.top, '460px')
})

const qoderText = '\x1b[38;2;216;216;216m\x1b[48;2;30;30;30m'
const qoderCaret = '\x1b[38;2;30;30;30m\x1b[48;2;216;216;216m'

test('Qoder ConPTY output anchors composition after Chinese text while the buffer cursor is parked at the right edge', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  // Preserve the cursor movement and inverse attributes emitted by Qoder CLI 1.1.78.
  await new Promise((resolve) => terminal.write('\x1b[?25l\x1b[14;2H> \x1b[7m \x1b[27m Type your message\x1b[K\x1b[80C', resolve))
  await new Promise((resolve) => terminal.write('\x1b[14;4H如何\x1b[7m \x1b[27m\x1b[1C\x1b[K\x1b[70C', resolve))
  assert.equal(terminal.buffer.active.cursorX, 79)
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  fixture.textarea.dispatchEvent(new Event('compositionupdate'))
  await new Promise((resolve) => setTimeout(resolve, 10))
  assert.equal(fixture.textarea.style.left, '70px')
  assert.equal(fixture.textarea.style.top, '260px')
  assert.equal(fixture.textarea.style.width, '10px')
  assert.equal(fixture.composition.style.left, '70px')
})

test('RGB cursor matches bold palette text according to drawBoldTextInBrightColors', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  for (const [bright, color] of [[true, '238;238;236'], [false, '211;215;207']]) {
    fixture.term.options.drawBoldTextInBrightColors = bright
    await new Promise((resolve) => terminal.write(`\x1b[0m\x1b[2J\x1b[?25l\x1b[3;4H\x1b[1;37;40mx\x1b[0;38;2;46;52;54;48;2;${color}m \x1b[1;37;40mx\x1b[5;35H`, resolve))
    fixture.listeners.get('onRender')()
    assert.equal(fixture.textarea.style.left, '40px', String(bright))
    assert.equal(fixture.textarea.style.top, '40px', String(bright))
  }
})

test('Qoder black-on-light cursor is recognized over the default dark background', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  fixture.term.options.theme = { foreground: '#f5f5f7', background: '#1c1c1e' }
  // Attributes captured from Qoder CLI: text uses RGB foreground/default background,
  // while the cursor uses black foreground rather than the terminal background.
  for (const char of [' ', 'a', '中']) {
    await new Promise((resolve) => terminal.write(`\x1b[0m\x1b[2J\x1b[?25l\x1b[14;2H> \x1b[38;2;238;238;235mhello\x1b[38;2;0;0;0;48;2;238;238;235m${char}\x1b[0m\x1b[K\x1b[70C`, resolve))
    fixture.textarea.dispatchEvent(new Event('compositionstart'))
    fixture.textarea.dispatchEvent(new Event('compositionupdate'))
    await new Promise((resolve) => setTimeout(resolve, 10))
    assert.equal(fixture.textarea.style.left, '80px', char)
    assert.equal(fixture.textarea.style.top, '260px', char)
    assert.equal(fixture.composition.style.left, '80px', char)
    fixture.textarea.dispatchEvent(new Event('compositionend'))
  }
})

test('Qoder RGB cursor anchors spaces, text and wide characters instead of the parked cursor', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  for (const char of [' ', 'a', '中']) {
    await new Promise((resolve) => terminal.write(`\x1b[2J\x1b[?25l\x1b[3;3H${qoderText}中${qoderCaret}${char}${qoderText} text\x1b[5;35H`, resolve))
    fixture.textarea.dispatchEvent(new Event('compositionstart'))
    assert.equal(terminal.buffer.active.getLine(2).getCell(4).isInverse(), 0)
    assert.equal(fixture.textarea.style.left, '40px', char)
    assert.equal(fixture.textarea.style.top, '40px', char)
    assert.equal(fixture.composition.style.left, '40px', char)
    fixture.textarea.dispatchEvent(new Event('compositionupdate'))
    await new Promise((resolve) => setTimeout(resolve, 10))
    assert.equal(fixture.textarea.style.left, '40px', char)
    assert.equal(fixture.textarea.style.width, '10px', char)
    fixture.textarea.dispatchEvent(new Event('compositionend'))
  }
})

test('RGB cursor compares default and palette colors through the current terminal theme', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  fixture.term.options.theme = { foreground: '#d8d8d8', background: '#1e1e1e', white: '#d8d8d8', black: '#1e1e1e' }
  for (const normal of ['\x1b[0m', '\x1b[37;40m', qoderText]) {
    await new Promise((resolve) => terminal.write(`\x1b[0m\x1b[2J\x1b[?25l\x1b[3;4H${normal}x${qoderCaret} ${normal}x\x1b[5;35H`, resolve))
    fixture.listeners.get('onRender')()
    assert.equal(fixture.textarea.style.left, '40px', normal)
    assert.equal(fixture.textarea.style.top, '40px', normal)
  }
  fixture.term.options.theme = { foreground: '#eeeeee', background: '#111111' }
  await new Promise((resolve) => terminal.write('\x1b[0m\x1b[2J\x1b[3;4Hx\x1b[38;2;17;17;17;48;2;238;238;238m \x1b[0mx\x1b[5;35H', resolve))
  fixture.listeners.get('onRender')()
  assert.equal(fixture.textarea.style.left, '40px')
})

test('RGB highlighting and ambiguous visual cursors fall back to the buffer cursor', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  for (const content of [
    `${qoderText}x${qoderCaret}AB${qoderText}x`,
    `${qoderText}x${qoderCaret}中A${qoderText}x`,
    `${qoderText}x\x1b[48;2;255;0;0mA${qoderText}x`,
    `${qoderText}x${qoderCaret} ${qoderText}x${qoderCaret} ${qoderText}x`,
    `${qoderText}x${qoderCaret} ${qoderText}x\x1b[6;5H\x1b[7m \x1b[0m`,
  ]) {
    await new Promise((resolve) => terminal.write(`\x1b[0m\x1b[2J\x1b[?25l\x1b[3;4H${content}\x1b[5;35H`, resolve))
    fixture.listeners.get('onRender')()
    assert.equal(fixture.textarea.style.left, '340px', content)
    assert.equal(fixture.textarea.style.top, '80px', content)
  }
})

test('RGB cursor resolves the default ANSI palette, color cube and grayscale', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  for (const [normal, caret] of [
    ['\x1b[37;40m', '\x1b[38;2;46;52;54;48;2;211;215;207m'],
    ['\x1b[38;5;75;48;5;235m', '\x1b[38;2;38;38;38;48;2;95;175;255m'],
    ['\x1b[38;5;250;48;5;16m', '\x1b[38;2;0;0;0;48;2;188;188;188m'],
  ]) {
    await new Promise((resolve) => terminal.write(`\x1b[0m\x1b[2J\x1b[?25l\x1b[3;4H${normal}x${caret} ${normal}x\x1b[5;35H`, resolve))
    fixture.listeners.get('onRender')()
    assert.equal(fixture.textarea.style.left, '40px', normal)
    assert.equal(fixture.textarea.style.top, '40px', normal)
  }
})

test('RGB cursor at either screen edge is ignored when visible or ambiguous during composition', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  fixture.term.options.theme = { foreground: '#d8d8d8', background: '#1e1e1e' }
  for (const [col, content] of [[0, `${qoderCaret} ${qoderText}x`], [79, `${qoderText}x${qoderCaret} `]]) {
    await new Promise((resolve) => terminal.write(`\x1b[0m\x1b[2J\x1b[?25l\x1b[3;${col === 0 ? 1 : 79}H${content}\x1b[5;35H`, resolve))
    fixture.textarea.dispatchEvent(new Event('compositionstart'))
    assert.equal(fixture.textarea.style.left, `${col * 10}px`)
    await new Promise((resolve) => terminal.write(`\x1b[7;4H${qoderText}x${qoderCaret} ${qoderText}x\x1b[5;35H`, resolve))
    fixture.listeners.get('onRender')()
    assert.equal(fixture.textarea.style.left, '340px')
    fixture.textarea.dispatchEvent(new Event('compositionend'))
  }
  await new Promise((resolve) => terminal.write(`\x1b[0m\x1b[2J\x1b[?25h\x1b[3;4H${qoderText}x${qoderCaret} ${qoderText}x\x1b[5;35H`, resolve))
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(fixture.textarea.style.left, '340px')
})

test('RGB cursor pin survives blinking, follows movement and releases on show or reset', async (t) => {
  for (const reset of ['\x1b[?25h', '\x1b[!p', '\x1bc', null]) {
    const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
    const fixture = imeFixture(terminal)
    try {
      await new Promise((resolve) => terminal.write(`\x1b[?25l\x1b[3;4H${qoderText}x${qoderCaret} ${qoderText}x\x1b[5;35H`, resolve))
      fixture.textarea.dispatchEvent(new Event('compositionstart'))
      assert.equal(fixture.textarea.style.left, '40px')
      await new Promise((resolve) => terminal.write(`\x1b[3;5H${qoderText} \x1b[5;35H`, resolve))
      fixture.listeners.get('onRender')()
      assert.equal(fixture.textarea.style.left, '40px')
      await new Promise((resolve) => terminal.write(`\x1b[3;6H${qoderCaret}x${qoderText}x\x1b[5;35H`, resolve))
      fixture.listeners.get('onRender')()
      assert.equal(fixture.textarea.style.left, '50px')
      fixture.screen.clientWidth = 640
      fixture.listeners.get('onResize')()
      assert.equal(fixture.textarea.style.left, '40px')
      if (reset) await new Promise((resolve) => terminal.write(reset, resolve))
      else { terminal.reset(); fixture.binding.reset() }
      await new Promise((resolve) => terminal.write('\x1b[5;35H', resolve))
      fixture.listeners.get('onRender')()
      assert.equal(fixture.textarea.style.left, '272px', String(reset))
      assert.equal(fixture.textarea.style.top, '80px', String(reset))
    } finally { fixture.binding.dispose(); terminal.dispose() }
  }
})

test('宽字符与其他反色字符相邻时仍按高亮条处理', async (t) => {
  const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
  const fixture = imeFixture(terminal)
  t.after(() => { fixture.binding.dispose(); terminal.dispose() })
  await new Promise((resolve) => terminal.write('\x1b[?25l\x1b[3;5H\x1b[7m中A\x1b[0m\x1b[24;1H', resolve))
  fixture.textarea.dispatchEvent(new Event('compositionstart'))
  assert.equal(fixture.textarea.style.left, '0px')
  assert.equal(fixture.textarea.style.top, '460px')
})

test('软重置、硬重置和重启会话均清除组词期间的假光标状态', async (t) => {
  for (const sequence of ['\x1b[!p', '\x1bc', null]) {
    const terminal = new xterm.Terminal({ cols: 80, rows: 24 })
    const fixture = imeFixture(terminal)
    try {
      await new Promise((resolve) => terminal.write('\x1b[?25l\x1b[3;5H\x1b[7m中\x1b[0m\x1b[24;1H', resolve))
      fixture.textarea.dispatchEvent(new Event('compositionstart'))
      assert.equal(fixture.textarea.style.left, '40px')
      if (sequence) await new Promise((resolve) => terminal.write(sequence, resolve))
      else { terminal.reset(); fixture.binding.reset() }
      // 留下新的反色输出，验证隐藏状态已清除，而非仅旧光标被清屏。
      await new Promise((resolve) => terminal.write('\x1b[H\x1b[7mX\x1b[0m\r\nPS> ', resolve))
      fixture.listeners.get('onRender')()
      assert.equal(fixture.textarea.style.left, '40px', String(sequence))
      assert.equal(fixture.textarea.style.top, '20px', String(sequence))
    } finally { fixture.binding.dispose(); terminal.dispose() }
  }
})

test('重绘、滚动、尺寸变化和焦点恢复时更新锚点，释放后移除监听', () => {
  const fixture = imeFixture()
  fixture.buffer.cursorX = 80 // 自动折行前的光标不得超出最后一列。
  fixture.listeners.get('onCursorMove')()
  assert.equal(fixture.textarea.style.left, '790px')
  fixture.buffer.cursorX = 3
  fixture.buffer.viewportY = 8
  fixture.listeners.get('onScroll')()
  assert.equal(fixture.textarea.style.top, '140px')
  fixture.screen.clientWidth = 640
  fixture.listeners.get('onResize')()
  assert.equal(fixture.textarea.style.left, '24px')
  fixture.buffer.cursorX = 4
  fixture.listeners.get('onRender')()
  assert.equal(fixture.textarea.style.left, '32px')
  fixture.buffer.cursorX = 5
  fixture.textarea.dispatchEvent(new Event('focus'))
  assert.equal(fixture.textarea.style.left, '40px')
  fixture.binding.dispose()
  assert.equal(fixture.listeners.size, 0)
  fixture.buffer.cursorX = 6
  fixture.textarea.dispatchEvent(new Event('focus'))
  assert.equal(fixture.textarea.style.left, '40px')
})


test('终端自定义粘贴即时生效，解绑后旧 Ctrl+V 交回 Shell', async () => {
  const settings = { shortcuts: {} }
  let reads = 0
  const fixture = keyboardFixture(true, async () => { reads++; return 'custom paste' }, [], settings)
  settings.shortcuts = { terminalPasteAuto: [{ code: 'KeyY', ctrlKey: true, altKey: false, shiftKey: true, metaKey: false }] }
  fixture.key({ key: 'v', code: 'KeyV', keyCode: 86, ctrlKey: true })
  await flush()
  assert.deepEqual(fixture.sent, ['\x16'])
  assert.equal(reads, 0)
  fixture.key({ key: 'y', code: 'KeyY', keyCode: 89, ctrlKey: true, shiftKey: true })
  await flush()
  assert.deepEqual(fixture.sent, ['\x16', 'custom paste'])
  assert.equal(reads, 1)
  fixture.term.dispose()
})
