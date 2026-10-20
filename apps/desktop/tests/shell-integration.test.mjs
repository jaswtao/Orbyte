/** OSC 安全与输入状态机回归。Author: jaswtao · Date: 2026-10-14 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
const { ShellIntegration } = await loadService('terminal/shellIntegration')
test('提示符后首个回车只开始一次，D 保存退出码', () => {
  const events = []; const integration = new ShellIntegration((e) => events.push(e))
  integration.input('\r'); integration.osc(133, 'A'); integration.osc(133, 'B'); integration.input('echo'); integration.input('\r'); integration.input('\r'); integration.osc(133, 'D;3')
  assert.deepEqual(events, [{ type: 'promptStart' }, { type: 'promptEnd' }, { type: 'commandStart' }, { type: 'commandEnd', exitCode: 3 }])
})
test('CMD 无退出码，OSC 9 与 7 解码中文目录', () => {
  const events = []; const integration = new ShellIntegration((e) => events.push(e))
  integration.osc(133, 'D'); integration.osc(9, '9;C:\\项目'); integration.osc(7, 'file://localhost/C:/%E9%A1%B9%E7%9B%AE')
  assert.deepEqual(events, [{ type: 'commandEnd', exitCode: undefined }, { type: 'cwd', path: 'C:\\项目' }, { type: 'cwd', path: 'C:/项目' }])
})
test('超长、控制字符、非法 URI 和非 file 协议被丢弃', () => {
  const events = []; const integration = new ShellIntegration((e) => events.push(e))
  for (const [code, text] of [[9, `9;${'a'.repeat(1025)}`], [9, '9;bad\npath'], [7, 'file:///bad%0apath'], [7, 'https://example.com'], [7, 'file:///bad%xx']]) assert.equal(integration.osc(code, text), true)
  assert.deepEqual(events, [])
})
test('重启清除待输入状态，OSC C 能开始命令', () => {
  const events = []; const integration = new ShellIntegration((e) => events.push(e))
  integration.osc(133, 'B'); integration.reset(); integration.input('\r'); integration.osc(133, 'C')
  assert.deepEqual(events.map((e) => e.type), ['promptEnd', 'commandStart'])
})
