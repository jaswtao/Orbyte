/** 布局树验证。Author: jaswtao · Date: 2026-10-13 */
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import ts from 'typescript'

const source = await readFile(new URL('../src/services/workspace/layout.ts', import.meta.url), 'utf8')
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } })
const { leaves, splitLeaf, removeFromLayout, setRatio, moveTab, renameEditorPaths } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`)
const terminal = (id) => ({ type: 'terminal', terminalConfigId: id })
const layout = (root) => ({ schemaVersion: 1, activeTabId: 'a', tabs: [{ id: 'a', title: 'A', root }, { id: 'b', title: 'B', root: terminal('b') }] })

test('嵌套拆分只改变目标叶子，保留原树和编辑器', () => {
  const editor = { type: 'editor', id: 'e', path: 'src/main.rs' }
  const root = splitLeaf(terminal('a'), 'a', editor, 'horizontal')
  const next = splitLeaf(root, 'e', terminal('c'), 'vertical')
  assert.equal(root.children[1], editor)
  assert.equal(next.children[1].direction, 'vertical')
  assert.deepEqual(leaves(next), [terminal('a'), editor, terminal('c')])
  assert.deepEqual(splitLeaf(root, 'missing', terminal('c'), 'vertical'), root)
})

test('关闭嵌套窗格折叠分屏，关闭最后叶子选择存活 Tab', () => {
  const root = splitLeaf(splitLeaf(terminal('a'), 'a', terminal('c'), 'vertical'), 'c', terminal('d'), 'horizontal')
  const original = layout(root)
  const next = removeFromLayout(original, 'c')
  assert.deepEqual(leaves(next.tabs[0].root), [terminal('a'), terminal('d')])
  assert.equal(leaves(original.tabs[0].root).length, 3)
  const collapsed = removeFromLayout(next, 'd')
  assert.deepEqual(collapsed.tabs[0].root, terminal('a'))
  const last = removeFromLayout(collapsed, 'a')
  assert.equal(last.activeTabId, 'b')
  assert.equal(last.tabs.length, 1)
  assert.deepEqual(removeFromLayout(last, 'b'), { schemaVersion: 1, activeTabId: null, tabs: [] })
})

test('比例只更新指定分屏并限制范围，Tab 重排保留激活项', () => {
  const root = splitLeaf(splitLeaf(terminal('a'), 'a', terminal('c'), 'horizontal'), 'c', terminal('d'), 'vertical')
  const next = setRatio(root, [1], 2)
  assert.equal(next.ratio, 0.5)
  assert.equal(next.children[1].ratio, 0.9)
  assert.equal(root.children[1].ratio, 0.5)
  assert.equal(setRatio(root, [], -1).ratio, 0.1)
  const reordered = moveTab(layout(root), 'b', 'a')
  assert.deepEqual(reordered.tabs.map((tab) => tab.id), ['b', 'a'])
  assert.equal(reordered.activeTabId, 'a')
})
test('重命名目录只更新其下的编辑器路径，保留窗格 ID 与相邻目录', () => {
  const root = splitLeaf({ type: 'editor', id: 'e', path: 'src/a.rs' }, 'e', { type: 'editor', id: 'other', path: 'src-other/a.rs' }, 'vertical')
  const original = layout(root)
  const renamed = renameEditorPaths(original, 'SRC', 'source')
  assert.equal(leaves(renamed.tabs[0].root)[0].id, 'e')
  assert.equal(leaves(renamed.tabs[0].root)[0].path, 'source/a.rs')
  assert.equal(leaves(renamed.tabs[0].root)[1].path, 'src-other/a.rs')
  assert.equal(leaves(original.tabs[0].root)[0].path, 'src/a.rs')
  const single = { schemaVersion: 1, activeTabId: 'a', tabs: [{ id: 'a', title: 'a.rs', root: { type: 'editor', id: 'e', path: 'src/a.rs' } }, { id: 'b', title: '自定义', root: { type: 'editor', id: 'f', path: 'src/a.rs' } }] }
  const titled = renameEditorPaths(single, 'src/a.rs', 'src/b.rs')
  assert.deepEqual(titled.tabs.map((tab) => tab.title), ['b.rs', '自定义'])
})
