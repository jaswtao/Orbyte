/** 翻译插值回归测试。Author: jaswtao · Date: 2026-10-16 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'

const { translate } = await loadService('../i18n/translate')

test('中英文翻译原样保留参数中的替换特殊字符', () => {
  const name = "$& $$ $` $'"
  assert.equal(translate('en', 'delete.question', { name }), `Delete “${name}”?`)
  assert.equal(translate('zh', 'delete.question', { name }), `确定删除「${name}」？`)
  assert.equal(translate('en', 'pane.directory', { path: 'C:/tmp/$$cache' }), 'Directory: C:/tmp/$$cache')
})

test('只替换模板占位符，参数内容不再参与插值', () => {
  const params = { project: '{message}', name: 'cmd', message: 'gap' }
  assert.equal(translate('en', 'terminal.warning', params), '{message} · cmd: gap')
  assert.equal(translate('zh', 'terminal.warning', params), '{message} · cmd：gap')
})

test('缺失参数保留占位符，空字符串与数字零正常插入', () => {
  assert.equal(translate('en', 'terminal.warning', { project: '', name: 0 }), ' · 0: {message}')
  assert.equal(translate('en', 'delete.question'), 'Delete “{name}”?')
  assert.equal(translate('en', 'delete.question', {}), 'Delete “{name}”?')
})
