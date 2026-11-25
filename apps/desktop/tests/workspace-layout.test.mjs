/** 布局树验证。Author: jaswtao · Date: 2026-10-13 */
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import vm from 'node:vm'
import ts from 'typescript'
import { ref, watch } from 'vue'

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
test('页签可按目标两侧插入，原位和无效落点保留原布局', () => {
  const original = layout(terminal('a'))
  original.tabs.push({ id: 'c', title: 'C', root: terminal('c') })
  const order = (value) => value.tabs.map((tab) => tab.id)
  assert.deepEqual(order(moveTab(original, 'a', 'c', 'after')), ['b', 'c', 'a'])
  assert.deepEqual(order(moveTab(original, 'c', 'a', 'before')), ['c', 'a', 'b'])
  assert.deepEqual(order(moveTab(original, 'c', 'a', 'after')), ['a', 'c', 'b'])
  assert.deepEqual(order(moveTab(original, 'a', 'c', 'before')), ['b', 'a', 'c'])
  assert.equal(moveTab(original, 'a', 'b', 'before'), original)
  assert.equal(moveTab(original, 'b', 'a', 'after'), original)
  assert.equal(moveTab(original, 'a', 'missing', 'after'), original)
  assert.deepEqual(order(original), ['a', 'b', 'c'])
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

test('WorkspaceView tab drag respects readiness, padding edges, external drags and template bindings', async () => {
  const source = await readFile(new URL('../src/components/workspace/WorkspaceView.vue', import.meta.url), 'utf8')
  const dragSource = await readFile(new URL('../src/components/workspace/useTabDrag.ts', import.meta.url), 'utf8')
  const wiringStart = source.indexOf('const { startTabDrag')
  const wiringEnd = source.indexOf('\n)', source.indexOf('= useTabDrag(', wiringStart)) + 2
  const script = dragSource.replace(/^import .*$/gm, '').replace('export function', 'function') + source.slice(wiringStart, wiringEnd)
  const workspaceLayout = ref({
    schemaVersion: 1,
    activeTabId: 'b',
    tabs: [
      { id: 'a', title: 'A', root: terminal('a') },
      { id: 'b', title: 'B', root: terminal('b') },
      { id: 'c', title: 'C', root: terminal('c') },
    ],
  })
  const ready = ref(false)
  const busy = ref(false)
  const committed = []
  const context = vm.createContext({
    ref, watch, layout: workspaceLayout, ready, busy, props: { active: true }, moveTab,
    perform: async (action) => { if (!busy.value && ready.value) await action() },
    commit: async (next) => { workspaceLayout.value = next; committed.push(next) },
  })
  vm.runInContext(ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context)
  const drag = vm.runInContext('({ startTabDrag, overTabBar, leaveTabBar, dropTab, tabDragClasses, endTabDrag })', context)
  const add = { matches: (selector) => selector === '.workspace-tab-add', getBoundingClientRect: () => ({ left: 132 }) }
  const bar = {
    contains: () => false,
    getBoundingClientRect: () => ({ left: -6, right: 180, top: -5, bottom: 37 }),
    querySelectorAll: (selector) => {
      assert.equal(selector, ':scope > [data-tab-id]')
      return workspaceLayout.value.tabs.map(({ id }, index) => ({
        dataset: { tabId: id },
        nextElementSibling: index === workspaceLayout.value.tabs.length - 1 ? add : null,
        getBoundingClientRect: () => ({ left: index * 44, right: index * 44 + 40, width: 40, top: 0, bottom: 32 }),
      }))
    },
  }
  const event = (clientX, clientY = 16) => ({
    currentTarget: bar, clientX, clientY, prevented: false,
    dataTransfer: { setData(type, value) { this.type = type; this.value = value } },
    preventDefault() { this.prevented = true },
  })
  const classes = (id) => Object.entries(drag.tabDragClasses(id)).filter(([, active]) => active).map(([name]) => name)

  // External drag without an active tab drag must not be intercepted.
  const externalOver = event(10)
  drag.overTabBar(externalOver)
  assert.equal(externalOver.prevented, false)
  const externalDrop = event(10)
  drag.dropTab(externalDrop)
  assert.equal(externalDrop.prevented, false)
  assert.equal(committed.length, 0)

  // Dragging is blocked while workspace is not ready.
  const blockedStart = event(10)
  drag.startTabDrag(blockedStart, 'c')
  assert.equal(blockedStart.prevented, true)
  assert.deepEqual(classes('c'), [])

  // Ready workspace snaps inside left and vertical tab-bar padding, rejects outside bounds and add button.
  ready.value = true
  const started = event(100)
  drag.startTabDrag(started, 'c')
  assert.equal(started.prevented, false)
  assert.equal(started.dataTransfer.type, 'application/x-orbyte-workspace-tab')
  assert.equal(started.dataTransfer.effectAllowed, 'move')

  const leftPaddingOver = event(-3, -2)
  drag.overTabBar(leftPaddingOver)
  assert.equal(leftPaddingOver.prevented, true)
  assert.equal(leftPaddingOver.dataTransfer.dropEffect, 'move')
  assert.deepEqual(classes('a'), ['workspace-tab-drop-before'])

  const outOfBoundsY = event(10, -6)
  drag.overTabBar(outOfBoundsY)
  assert.equal(outOfBoundsY.prevented, false)
  assert.deepEqual(classes('a'), [])

  const outOfBoundsX = event(-7, 16)
  drag.overTabBar(outOfBoundsX)
  assert.equal(outOfBoundsX.prevented, false)
  assert.deepEqual(classes('a'), [])

  const overAddButton = event(140, 16)
  drag.overTabBar(overAddButton)
  assert.equal(overAddButton.prevented, false)
  assert.deepEqual(classes('c'), ['workspace-tab-dragging'])

  // Dropping in the left padding moves the tab before the first tab and commits via perform/commit.
  const dropLeft = event(-3, 35)
  drag.dropTab(dropLeft)
  assert.equal(dropLeft.prevented, true)
  assert.deepEqual(workspaceLayout.value.tabs.map((tab) => tab.id), ['c', 'a', 'b'])
  assert.equal(committed.length, 1)
  assert.deepEqual(classes('a'), [])
  assert.deepEqual(classes('c'), [])

  // Becoming not ready mid-drag clears drag classes immediately.
  drag.startTabDrag(event(10), 'c')
  drag.overTabBar(event(120))
  assert.deepEqual(classes('b'), ['workspace-tab-drop-after'])
  ready.value = false
  assert.deepEqual(classes('b'), [])
  assert.deepEqual(classes('c'), [])

  assert.match(source, /@dragenter="overTabBar" @dragover="overTabBar" @dragleave="leaveTabBar" @drop="dropTab"/)
  assert.match(source, /:data-tab-id="tab\.id" :draggable="ready && !busy"/)
  assert.match(source, /@dragstart="startTabDrag\(\$event, tab\.id\)" @dragend="endTabDrag"/)
  assert.match(source, /tabDragClasses\(tab\.id\)/)
})
