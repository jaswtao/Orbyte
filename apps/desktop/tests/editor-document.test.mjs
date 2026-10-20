/** 文档基线和未保存计数。Author: jaswtao · Date: 2026-10-13 */
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import ts from 'typescript'
const source = await readFile(new URL('../src/services/editor/document.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } })
const { DocumentState, unsavedCount } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
test('撤销到磁盘基线消除未保存状态，保存期间的新输入保持未保存', () => {
  const document = new DocumentState(); document.load({ content: 'initial', version: '1' })
  document.content = 'edited'; assert.equal(document.dirty, true)
  document.content = 'initial'; assert.equal(document.dirty, false)
  document.content = 'later input'; document.saved({ content: 'edited', version: '2' }); assert.equal(document.dirty, true)
  document.content = 'edited'; assert.equal(document.dirty, false)
})
test('自己的保存事件忽略，外部编辑区分自动刷新与冲突', () => {
  const document = new DocumentState(); document.load({ content: 'initial', version: '1' })
  assert.equal(document.changed({ content: 'initial', version: '1' }), 'ignore')
  assert.equal(document.changed({ content: 'external', version: '2' }), 'reload')
  document.content = 'mine'; assert.equal(document.changed({ content: 'external', version: '2' }), 'conflict'); assert.equal(document.external, true)
  document.load({ content: 'external', version: '2' }); assert.equal(document.dirty, false); assert.equal(document.external, false)
})
test('跨项目未保存计数涵盖隐藏文档，关闭与保存后归零', () => {
  const documents = new Map([['a', new DocumentState()], ['b', new DocumentState()]])
  documents.get('a').content = 'a'; documents.get('b').content = 'b'; assert.equal(unsavedCount(documents.values()), 2)
  documents.delete('a'); assert.equal(unsavedCount(documents.values()), 1)
  documents.get('b').saved({ content: 'b', version: '1' }); assert.equal(unsavedCount(documents.values()), 0)
})
