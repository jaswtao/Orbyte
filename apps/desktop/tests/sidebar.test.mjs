/** 侧栏行为回归。Author: jaswtao · Date: 2026-10-14 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { computed, nextTick, reactive, ref, watch } from 'vue'
import { loadService } from './load-service.mjs'
const { buildMenuOptions, setLocale } = await loadService('sidebar/menu')
const { dragPayload, parseDragPayload, reorderGroupIds, reorderProjectEntries } = await loadService('sidebar/reorder')
const { filterProjects, firstVisibleProject } = await loadService('sidebar/filter')
const target = { kind: 'project', id: 'a' }
test('空目标没有菜单', () => assert.deepEqual(buildMenuOptions(null, [], null, false), []))
test('分组菜单可添加项目、重命名和删除', () => assert.deepEqual(buildMenuOptions({ kind: 'group', id: 'g' }, [], null, false).map((o) => [o.key, o.label]), [['add-project', 'Add project'], ['rename', 'Rename group'], ['delete', 'Delete group']]))
test('收藏文案跟随项目状态', () => { for (const favorite of [false, true]) assert.equal(buildMenuOptions(target, [{ id: 'a', favorite }], null, false).find((o) => o.key === 'favorite').label, favorite ? 'Remove favorite' : 'Favorite project') })

test('隐藏菜单跟随项目状态并支持中英文', () => {
  for (const language of ['en', 'zh']) {
    setLocale(language)
    try {
      for (const hidden of [false, true]) {
        const label = buildMenuOptions(target, [{ id: 'a', hidden }], null, false).find((o) => o.key === 'hidden').label
        assert.equal(label, language === 'zh' ? (hidden ? '取消隐藏' : '隐藏项目') : (hidden ? 'Unhide project' : 'Hide project'))
      }
    } finally { setLocale('en') }
  }
})

const visibilityProjects = [
  { id: 'hidden', name: 'Alpha hidden', path: 'C:/hidden', groupId: 'a', favorite: true, hidden: true },
  { id: 'other', name: 'Other', path: 'C:/other', groupId: 'b', favorite: false, hidden: false },
  { id: 'visible', name: 'Alpha visible', path: 'C:/visible', groupId: 'a', favorite: true, hidden: false },
  { id: 'loose', name: 'Loose', path: 'C:/loose', groupId: null, favorite: true, hidden: false },
]

test('隐藏过滤与搜索、收藏组合，保留原始顺序和数组', () => {
  const original = structuredClone(visibilityProjects)
  assert.deepEqual(filterProjects(visibilityProjects, false).map((p) => p.id), ['other', 'visible', 'loose'])
  assert.deepEqual(filterProjects(visibilityProjects, true, true, 'ALPHA').map((p) => p.id), ['hidden', 'visible'])
  assert.deepEqual(filterProjects(visibilityProjects, false, true, 'ALPHA').map((p) => p.id), ['visible'])
  assert.deepEqual(filterProjects(visibilityProjects, false, false, 'C:/other').map((p) => p.id), ['other'])
  assert.deepEqual(visibilityProjects, original)
})

test('替代项目按分组顺序选择，排除目标和隐藏项目，支持无候选', () => {
  assert.equal(firstVisibleProject(visibilityProjects, ['a', 'b', ''], 'hidden', false, '').id, 'visible')
  assert.equal(firstVisibleProject(visibilityProjects, ['b', 'a', ''], 'hidden', false, '').id, 'other')
  assert.equal(firstVisibleProject(visibilityProjects, ['a', 'b', ''], 'visible', true, '').id, 'loose')
  assert.equal(firstVisibleProject(visibilityProjects, ['a', 'b', ''], 'hidden', false, 'missing'), undefined)
  assert.equal(firstVisibleProject(visibilityProjects, ['a', 'b', ''], 'visible', true, 'Alpha'), undefined)
})

test('拖拽完整列表时保留隐藏项目', () => {
  const ordered = reorderProjectEntries(visibilityProjects, 'visible', 'b', 'other')
  assert.equal(ordered.length, visibilityProjects.length)
  assert.ok(ordered.some(([id]) => id === 'hidden'))
  assert.equal(visibilityProjects[0].hidden, true)
})

const sidebarSource = await readFile(new URL('../src/components/sidebar/ProjectSidebar.vue', import.meta.url), 'utf8')
const sidebarScript = sidebarSource.split('<script setup lang="ts">')[1].split('</script>')[0].replace(/^import .*$/gm, '')
const sidebarJs = ts.transpileModule(sidebarScript, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText

function sidebarFixture(activeId = 'visible', fail = false) {
  const calls = []
  const observers = [], frames = new Map(), cleanup = []
  let frameId = 0
  const projects = structuredClone(visibilityProjects)
  const store = reactive({
    projects, groups: [{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }],
    active: projects.find((p) => p.id === activeId), settings: { show_hidden_projects: true },
    busy: false, ready: true,
    async run(action) { try { await action(); return true } catch { return false } },
  })
  const context = {
    computed, reactive, ref, t: (key) => key, filterProjects, firstVisibleProject,
    watch: (...args) => { const stop = watch(...args); cleanup.push(stop); return stop },
    onBeforeUnmount: (fn) => cleanup.push(fn),
    ResizeObserver: class {
      targets = []
      disconnected = false
      constructor(callback) { this.notify = callback; observers.push(this) }
      observe(target) { this.targets.push(target) }
      disconnect() { this.disconnected = true }
    },
    requestAnimationFrame: (fn) => { frames.set(++frameId, fn); return frameId },
    cancelAnimationFrame: (id) => frames.delete(id),
    useProjects: () => store, useGit: () => ({ available: false }),
    defineProps() {}, defineEmits: () => () => {}, defineExpose() {},
    projectApi: {
      async setProjectHidden(...args) { calls.push(['hidden', ...args]); if (fail) throw new Error('write failed') },
      async setSettings(input) { calls.push(['settings', structuredClone(input)]); if (fail) throw new Error('write failed') },
    },
  }
  vm.createContext(context)
  vm.runInContext(sidebarJs, context)
  return { calls, store, observers, frames, unmount: () => cleanup.forEach((fn) => fn()),
    flushFrames() { const pending = [...frames.values()]; frames.clear(); pending.forEach((fn) => fn()) },
    evaluate: (code) => vm.runInContext(code, context) }
}

test('定位项目仅按超出列表可视区域的距离滚动', () => {
  for (const [top, bottom, expected] of [[80, 130, 178], [70, 90, 168], [280, 330, 228], [320, 370, 268], [120, 170, 200], [102, 302, 200]]) {
    const fixture = sidebarFixture()
    const row = { dataset: { projectId: 'visible' }, getClientRects: () => [1], getBoundingClientRect: () => ({ top, bottom, height: bottom - top }) }
    const list = { scrollTop: 200, clientTop: 2, clientHeight: 200,
      getBoundingClientRect: () => ({ top: 100 }), querySelectorAll: () => [row] }
    fixture.store.list = list
    fixture.evaluate('projectList.value = store.list; revealProject("visible")')
    assert.equal(list.scrollTop, expected)
    fixture.evaluate('revealProject("missing")')
    assert.equal(list.scrollTop, expected)
    fixture.evaluate('projectList.value = null; revealProject("visible")')
  }
})

function revealFixture(t) {
  const fixture = sidebarFixture()
  t.after(fixture.unmount)
  const rows = ['visible', 'other'].map((id, index) => ({
    dataset: { projectId: id }, top: 250 + index * 60, height: 50, shown: true,
    getClientRects() { return this.shown ? [1] : [] },
    getBoundingClientRect() { const top = this.top - list.scrollTop; return { top, bottom: top + this.height, height: this.height } },
  }))
  const list = { scrollTop: 0, clientTop: 0, clientHeight: 200,
    getBoundingClientRect: () => ({ top: 100 }), querySelectorAll: () => rows, contains: (row) => rows.includes(row) }
  fixture.store.list = list
  fixture.evaluate('projectList.value = store.list; revealProject("visible")')
  return { ...fixture, rows, list }
}

test('Selected project stays fully visible when async Git information increases its height', (t) => {
  const fixture = revealFixture(t)
  const { rows, list, observers, frames } = fixture
  assert.equal(list.scrollTop, 0)
  rows[0].height += 20
  observers[0].notify()
  observers[0].notify()
  assert.equal(frames.size, 1)
  fixture.flushFrames()
  assert.equal(list.scrollTop, 20)
  observers[0].notify()
  fixture.flushFrames()
  assert.equal(list.scrollTop, 20)
  list.clientHeight -= 30
  observers[0].notify()
  fixture.flushFrames()
  assert.equal(list.scrollTop, 50)
  assert.equal(rows[0].getBoundingClientRect().bottom, 100 + list.clientHeight)
})

for (const pendingResize of [false, true]) {
  test(`Manual scrolling stops reveal tracking with pending resize: ${pendingResize}`, (t) => {
    const fixture = revealFixture(t)
    const { rows, list, observers, frames } = fixture
    if (pendingResize) observers[0].notify()
    list.scrollTop = 200
    if (!pendingResize) fixture.evaluate('onProjectListScroll()')
    rows[0].height += 20
    list.clientHeight -= 30
    observers[0].notify()
    fixture.flushFrames()
    assert.equal(list.scrollTop, 200)
    assert.equal(observers[0].disconnected, true)
    assert.equal(frames.size, 0)
    fixture.evaluate('revealProject("visible")')
    assert.equal(list.scrollTop, 150)
    assert.equal(observers.at(-1).disconnected, false)
  })
}

test('Programmatic reveal scroll events preserve tracking for later Git updates', (t) => {
  const fixture = revealFixture(t)
  const { rows, list, observers } = fixture
  rows[0].height += 20
  observers[0].notify()
  fixture.flushFrames()
  assert.equal(list.scrollTop, 20)
  fixture.evaluate('onProjectListScroll()')
  assert.equal(observers[0].disconnected, false)
  rows[0].height += 20
  observers[0].notify()
  fixture.flushFrames()
  assert.equal(list.scrollTop, 40)
})

test('Switching targets cancels old callbacks and reveals rows above or below the viewport', async (t) => {
  const fixture = revealFixture(t)
  const { list, observers, frames } = fixture
  observers[0].notify()
  fixture.store.active = fixture.store.projects.find((project) => project.id === 'other')
  fixture.evaluate('revealProject("other")')
  await nextTick()
  assert.equal(observers[0].disconnected, true)
  assert.equal(frames.size, 0)
  assert.equal(list.scrollTop, 60)
  observers[0].notify()
  assert.equal(frames.size, 0)
  fixture.store.active = fixture.store.projects.find((project) => project.id === 'visible')
  fixture.evaluate('revealProject("visible")')
  fixture.rows[0].top = 150
  observers.at(-1).notify()
  fixture.flushFrames()
  assert.equal(list.scrollTop, 50)
})

for (const reason of ['active', 'filter', 'collapse', 'unmount']) {
  test(`Reveal tracking is cleaned up after ${reason}`, async (t) => {
    const fixture = revealFixture(t)
    fixture.observers[0].notify()
    if (reason === 'active') fixture.store.active = fixture.store.projects[0]
    else if (reason === 'filter') fixture.evaluate('search.value = "missing"')
    else if (reason === 'collapse') fixture.evaluate('collapsed.add("a")')
    else fixture.unmount()
    await nextTick()
    assert.equal(fixture.observers[0].disconnected, true)
    assert.equal(fixture.frames.size, 0)
    fixture.observers[0].notify()
    assert.equal(fixture.frames.size, 0)
  })
}

test('Oversized rows align at the top without oscillation; hidden rows stop tracking', (t) => {
  const fixture = revealFixture(t)
  const { rows, list, observers } = fixture
  rows[0].height = 250
  observers[0].notify()
  fixture.flushFrames()
  assert.equal(list.scrollTop, 150)
  observers[0].notify()
  fixture.flushFrames()
  assert.equal(list.scrollTop, 150)
  rows[0].shown = false
  observers[0].notify()
  fixture.flushFrames()
  assert.equal(observers[0].disconnected, true)
})

test('隐藏当前项目按筛选选择替代项目，成功后展开目标分组', async () => {
  const fixture = sidebarFixture()
  fixture.evaluate("collapsed.add('b')")
  await fixture.evaluate('toggleHidden(store.active)')
  assert.deepEqual(fixture.calls, [['hidden', 'visible', true, 'other']])
  assert.equal(fixture.evaluate("collapsed.has('b')"), false)
  const filtered = sidebarFixture()
  filtered.evaluate('favoritesOnly.value = true')
  await filtered.evaluate('toggleHidden(store.active)')
  assert.deepEqual(filtered.calls, [['hidden', 'visible', true, 'loose']])
})

test('关闭显示开关与活动项目切换一次提交，无候选时清空工作区', async () => {
  const fixture = sidebarFixture('hidden')
  await fixture.evaluate('setShowHidden(false)')
  assert.deepEqual(fixture.calls, [['settings', { show_hidden_projects: false, active_project_id: 'visible' }]])
  const empty = sidebarFixture('hidden')
  empty.evaluate("search.value = 'missing'")
  await empty.evaluate('setShowHidden(false)')
  assert.deepEqual(empty.calls, [['settings', { show_hidden_projects: false, active_project_id: null }]])
  const hide = sidebarFixture()
  hide.evaluate("search.value = 'Alpha'")
  await hide.evaluate('toggleHidden(store.active)')
  assert.deepEqual(hide.calls, [['hidden', 'visible', true, null]])
})

test('取消隐藏和普通显示切换不激活项目，写入失败保留原视图', async () => {
  const fixture = sidebarFixture('hidden')
  await fixture.evaluate('toggleHidden(store.active)')
  await fixture.evaluate('setShowHidden(true)')
  assert.deepEqual(fixture.calls, [['hidden', 'hidden', false, null], ['settings', { show_hidden_projects: true }]])
  const failed = sidebarFixture('hidden', true)
  failed.evaluate("collapsed.add('a')")
  await failed.evaluate('setShowHidden(false)')
  assert.equal(failed.evaluate("collapsed.has('a')"), true)
  assert.equal(failed.store.settings.show_hidden_projects, true)
  assert.equal(failed.store.active.id, 'hidden')
  assert.equal(failed.store.active.hidden, true)
})
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
