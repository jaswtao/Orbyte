/** 侧栏行为回归。Author: jaswtao · Date: 2026-10-14 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
const { buildMenuOptions, setLocale } = await loadService('sidebar/menu')
const { dragPayload, parseDragPayload, reorderGroupIds, reorderProjectEntries } = await loadService('sidebar/reorder')
const target = { kind: 'project', id: 'a' }
test('空目标没有菜单', () => assert.deepEqual(buildMenuOptions(null, [], null, false), []))
test('分组菜单可添加项目、重命名和删除', () => assert.deepEqual(buildMenuOptions({ kind: 'group', id: 'g' }, [], null, false).map((o) => [o.key, o.label]), [['add-project', 'Add project'], ['rename', 'Rename group'], ['delete', 'Delete group']]))
test('收藏文案跟随项目状态', () => { for (const favorite of [false, true]) assert.equal(buildMenuOptions(target, [{ id: 'a', favorite }], null, false).find((o) => o.key === 'favorite').label, favorite ? 'Remove favorite' : 'Favorite project') })
test('信息栏文案跟随设置', () => { for (const show of [false, true]) assert.equal(buildMenuOptions(target, [], { show_workspace_header: show }, false)[0].label, show ? 'Hide workspace header' : 'Show workspace header') })
test('切换中文后菜单使用中文', () => {
  setLocale('zh')
  try {
    assert.deepEqual(buildMenuOptions({ kind: 'group', id: 'g' }, [], null, false).map((o) => o.label), ['添加项目', '重命名分组', '删除分组'])
  } finally { setLocale('en') }
})
test('忙碌时禁用所有操作', () => assert.ok(buildMenuOptions(target, [], null, true).every((o) => o.disabled)))
test('组前移且保留原数组', () => { const ids = ['a', 'b', 'c']; assert.deepEqual(reorderGroupIds(ids, 'c', 'a'), ['c', 'a', 'b']); assert.deepEqual(ids, ['a', 'b', 'c']) })
const projects = [{ id: 'a', groupId: 'x' }, { id: 'b', groupId: 'x' }, { id: 'c', groupId: 'y' }]
test('项目组内前移', () => assert.deepEqual(reorderProjectEntries(projects, 'b', 'x', 'a'), [['b', 'x'], ['a', 'x'], ['c', 'y']]))
test('跨组拖拽', () => assert.deepEqual(reorderProjectEntries(projects, 'a', 'y', 'c'), [['b', 'x'], ['a', 'y'], ['c', 'y']]))
test('拖到组尾与未分组', () => assert.deepEqual(reorderProjectEntries(projects, 'a', null), [['b', 'x'], ['c', 'y'], ['a', null]]))
test('自身或失效目标不改变顺序', () => { for (const before of ['a', 'missing']) assert.deepEqual(reorderProjectEntries(projects, 'a', 'y', before), projects.map((p) => [p.id, p.groupId])) })
test('拖拽载荷只接受项目和分组', () => {
  assert.deepEqual(parseDragPayload(dragPayload('project', 'a')), { kind: 'project', id: 'a' })
  assert.deepEqual(parseDragPayload(dragPayload('group', 'g:1')), { kind: 'group', id: 'g:1' })
  for (const raw of ['', 'a', 'file:a', 'project:']) assert.equal(parseDragPayload(raw), null)
})
