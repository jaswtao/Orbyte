/** 标记优先级回归。Author: jaswtao · Date: 2026-10-14 */
import assert from 'node:assert/strict'
import test from 'node:test'
import xterm from '@xterm/xterm'
import { loadService } from './load-service.mjs'
const { activity } = await loadService('workspace/activity')
const { ContentActivity } = await loadService('terminal/contentActivity')
const entry = () => ({ status: { state: 'running' }, shell: { command: 'idle', lastExitCode: null }, unread: true })
test('未读输出与已读输出', () => { const e = entry(); assert.equal(activity([e]), 'unread'); e.unread = false; assert.equal(activity([e]), null); assert.equal(activity([]), null) })
test('运行命令不显示运行标记，保留未读与失败提示', () => { const running = entry(); running.shell.command = 'running'; running.unread = false; assert.equal(activity([running]), null); assert.equal(activity([entry(), running]), 'unread'); assert.equal(activity([running, { ...entry(), status: { state: 'failed' } }]), 'failed') })
test('非零会话退出码为失败，正常退出仍可未读', () => { const e = entry(); e.status = { state: 'exited', exitCode: 1 }; assert.equal(activity([e]), 'failed'); e.status.exitCode = 0; assert.equal(activity([e]), 'unread') })
test('失败命令不受后续输出影响，保持到新命令开始', () => { const e = entry(); e.shell.lastExitCode = 3; assert.equal(activity([e]), 'failed'); e.unread = false; assert.equal(activity([e]), 'failed'); e.shell = { command: 'running', lastExitCode: null }; assert.equal(activity([e]), null) })

function contentFixture(t, integrated = false) {
  const term = new xterm.Terminal({ cols: 40, rows: 5, scrollback: 20 })
  t.after(() => term.dispose())
  const detector = new ContentActivity()
  term.onResize(() => detector.resized(term.buffer.active))
  let output = false
  if (integrated) term.parser.registerOscHandler(133, (data) => {
    const kind = data.split(';')[0]
    if (kind === 'A') { output = detector.changed(term.buffer.active) || output; detector.promptStart(term) }
    if (kind === 'B') detector.promptEnd(term)
    if (kind === 'C') detector.commandStart(term)
    return true
  })
  return {
    term, detector,
    changed: () => { const changed = detector.changed(term.buffer.active) || output; output = false; return changed },
    write: (text) => new Promise((resolve) => term.write(text, resolve)),
  }
}

const osc = (kind) => `\x1b]133;${kind}\x07`
const prompt = (path = 'C:\\Orbyte') => `${osc('A')}PS ${path}> ${osc('B')}`

test('集成提示符首次出现、分段到达及原位重绘不触发未读', async (t) => {
  const f = contentFixture(t, true)
  for (const chunk of [osc('A'), 'PS C:\\', 'Orbyte> ', osc('B')]) {
    await f.write(chunk)
    assert.equal(f.changed(), false)
  }
  await f.write('\r\x1b[K' + prompt())
  assert.equal(f.changed(), false)
  await f.write('\x1b[?25l\x1b[32m\x1b[0m\x1b[?25h')
  assert.equal(f.changed(), false)
})

test('空回车、目录切换和长提示符输入回显不触发未读', async (t) => {
  const f = contentFixture(t, true)
  await f.write(prompt())
  assert.equal(f.changed(), false)
  await f.write('\r\n' + prompt())
  assert.equal(f.changed(), false)
  await f.write('cd 子目录\r\n' + prompt('C:\\' + 'long-path\\'.repeat(8)))
  assert.equal(f.changed(), false)
  f.term.resize(30, 8)
  assert.equal(f.changed(), false)
  await f.write('cd ..\r\n' + prompt())
  assert.equal(f.changed(), false)
})

test('真实输出与提示符同批到达、无换行输出和启动错误均保留提醒', async (t) => {
  const f = contentFixture(t, true)
  await f.write('启动错误\r\n' + prompt())
  assert.equal(f.changed(), true)
  assert.equal(f.changed(), false)
  await f.write('echo hello\r\nhello\r\n' + prompt())
  assert.equal(f.changed(), true)
  await f.write('echo hello\r\nhello' + prompt())
  assert.equal(f.changed(), true)
  assert.equal(f.changed(), false)
  await f.write('\r\n' + prompt())
  assert.equal(f.changed(), false)
})

test('提示符过滤不掩盖原行被真实输出覆盖及备用缓冲区变化', async (t) => {
  const f = contentFixture(t, true)
  await f.write(prompt())
  assert.equal(f.changed(), false)
  await f.write('\r\x1b[K真实内容')
  assert.equal(f.changed(), true)
  await f.write('\x1b[?1049h备用屏幕内容')
  assert.equal(f.changed(), true)
  await f.write('\r\x1b[K备用屏幕更新')
  assert.equal(f.changed(), true)
})

test('显式命令开始后同一行的真实输出不被过滤', async (t) => {
  const f = contentFixture(t, true)
  await f.write(prompt() + 'command' + osc('C') + 'result')
  assert.equal(f.changed(), true)
})

test('集成终端后台输出滚动、原位修改及清屏仍触发未读', async (t) => {
  const f = contentFixture(t, true)
  await f.write(prompt())
  assert.equal(f.changed(), false)
  await f.write('command\r\n' + '真实输出\r\n'.repeat(30) + prompt())
  assert.equal(f.changed(), true)
  await f.write('command\r\n处理中')
  assert.equal(f.changed(), true)
  await f.write('\r处理完成')
  assert.equal(f.changed(), true)
  await f.write('\r\x1b[K')
  assert.equal(f.changed(), true)
  await f.write('\x1b[2J\x1b[H')
  assert.equal(f.changed(), true)
})

test('无集成标记保留原有检测，重置后不沿用旧提示符状态', async (t) => {
  const plain = contentFixture(t)
  await plain.write('PS C:\\Orbyte> ')
  assert.equal(plain.changed(), true)
  const f = contentFixture(t, true)
  await f.write(prompt())
  assert.equal(f.changed(), false)
  f.detector.reset(f.term.buffer.active)
  await f.write('\r\n' + prompt())
  assert.equal(f.changed(), false)
  await f.write('\r\n真实输出')
  assert.equal(f.changed(), true)
})

test('空闲终端的光标、颜色、标题及相同内容重绘不触发未读', async (t) => {
  const f = contentFixture(t)
  assert.equal(f.changed(), false)
  await f.write('Codex 等待输入')
  assert.equal(f.changed(), true)
  await f.write('\x1b[?25l\x1b[31m\x1b[0m\x1b]0;新标题\x07\x1b[?25h')
  assert.equal(f.changed(), false)
  await f.write('\x1b[2J\x1b[HCodex 等待输入')
  assert.equal(f.changed(), false)
})

test('后台分批清屏重绘恢复已读内容后撤销未读', async (t) => {
  const f = contentFixture(t)
  await f.write('Codex waiting for input')
  f.changed()
  f.detector.markRead()
  await f.write('\x1b[2J\x1b[H')
  f.changed()
  assert.equal(f.detector.unread, true)
  await f.write('Codex waiting for input')
  f.changed()
  assert.equal(f.detector.unread, false)
})

test('未读与最后已读内容比较，重复检查不会吞掉真实输出', async (t) => {
  const f = contentFixture(t)
  await f.write('seen output')
  f.changed()
  f.detector.markRead()
  await f.write('\r\nnew output')
  f.changed()
  assert.equal(f.detector.unread, true)
  assert.equal(f.changed(), false)
  assert.equal(f.detector.unread, true)
  f.detector.markRead()
  assert.equal(f.detector.unread, false)
  await f.write('\r\nmore output')
  f.changed()
  assert.equal(f.detector.unread, true)
  f.detector.reset(f.term.buffer.active)
  assert.equal(f.detector.unread, false)
})

test('新增输出、原位修改、删除和清屏均识别为内容变化', async (t) => {
  const f = contentFixture(t)
  for (const text of ['处理中', '\r处理完成', '\r\x1b[K', '新内容\r\n下一行', '\x1b[2J\x1b[H']) {
    await f.write(text)
    assert.equal(f.changed(), true, JSON.stringify(text))
    assert.equal(f.changed(), false)
  }
})

test('后台输出滚入历史缓冲后仍能识别', async (t) => {
  const f = contentFixture(t)
  await f.write('首行\r\n' + '重复行\r\n'.repeat(7))
  assert.equal(f.changed(), true)
  await f.write('重复行\r\n')
  assert.equal(f.changed(), true)
})

test('软换行与空白视口尺寸变化不触发提示', async (t) => {
  const f = contentFixture(t)
  await f.write('abcdefghijklmnopqrstuvwxyz'.repeat(3) + '\r\n')
  assert.equal(f.changed(), true)
  f.term.resize(30, 8)
  assert.equal(f.changed(), false)
})
