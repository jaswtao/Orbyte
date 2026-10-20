/** 通知策略边界验证。Author: jaswtao · Date: 2026-10-14 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
import { readFile } from 'node:fs/promises'
import ts from 'typescript'
import { watch } from 'vue'
const { shouldNotify, notificationAllowed } = await loadService('workspace/notificationPolicy')
test('后台长命令与失焦窗口通知，当前可见终端不通知', () => {
  assert.equal(shouldNotify(5, 6000, false, true), true)
  assert.equal(shouldNotify(5, 6000, true, false), true)
  assert.equal(shouldNotify(5, 6000, true, true), false)
  assert.equal(shouldNotify(5, 4999, false, true), false)
  assert.equal(shouldNotify(5, 5000, false, true), true)
})
test('自然退出无需达到长命令阈值，0 关闭全部通知', () => {
  assert.equal(shouldNotify(30, null, false, true), true)
  assert.equal(shouldNotify(0, null, false, false), false)
  assert.equal(shouldNotify(0, 999999, false, false), false)
})
test('同终端限流 5 秒，首次通知不限流', () => {
  assert.equal(notificationAllowed(undefined, 0), true)
  assert.equal(notificationAllowed(1000, 5999), false)
  assert.equal(notificationAllowed(1000, 6000), true)
})

test('工作区消息连续同文与不同级别均逐次送达，空消息不弹出', async () => {
  // data: 模块需用绝对 URL 引用真实 Vue，验证同步响应式事件而非模拟响应式。
  const source = (await readFile(new URL('../src/services/workspace/messages.ts', import.meta.url), 'utf8'))
    .replace("from 'vue'", `from '${import.meta.resolve('vue')}'`)
  const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } })
  const { workspaceMessage, showWorkspaceMessage } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
  const received = []
  const stop = watch(workspaceMessage, (message) => received.push(message), { flush: 'sync' })
  try {
    showWorkspaceMessage('复制失败')
    showWorkspaceMessage('复制失败')
    showWorkspaceMessage('项目 A · PowerShell：输出存在缺口', 'warning')
    showWorkspaceMessage('')
    assert.deepEqual(received, [
      { type: 'error', content: '复制失败' },
      { type: 'error', content: '复制失败' },
      { type: 'warning', content: '项目 A · PowerShell：输出存在缺口' },
    ])
    assert.notEqual(received[0], received[1], '相同消息也必须触发新的响应式事件')
  } finally { stop() }
})
