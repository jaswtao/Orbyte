<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import UiIcon from '../UiIcon.vue'
import { NButton, NDropdown, NInput, NModal, type DropdownOption } from 'naive-ui'
import type { Project, Settings } from '../../services/projectApi'
import { terminalApi, type ShellInfo, type ShellType, type TerminalConfig, type LaunchPreset } from '../../services/terminalApi'
import { INITIAL_SIZE } from '../../services/terminal/terminalClient'
import type { TerminalRegistry, TerminalEntry } from '../../services/terminal/registry'
import { workspaceApi } from '../../services/workspaceApi'
import { emptyLayout, leaves, moveTab, removeFromLayout, setRatio, splitLeaf, type Direction, type Layout, type WorkspaceTab } from '../../services/workspace/layout'
import { matchShortcut, shortcutHint } from '../../services/workspace/shortcuts'
import { activity, type Activity } from '../../services/workspace/activity'
import { notify } from '../../services/notify'
import { shouldNotify } from '../../services/workspace/notificationPolicy'
import { showWorkspaceMessage } from '../../services/workspace/messages'
import { exportBuffer, outputFilename } from '../../services/terminal/export'
import { useGit } from '../../stores/git'
import LayoutPane from './LayoutPane.vue'
import QuickOpen from '../files/QuickOpen.vue'
import type { EditorRegistry } from '../../services/editor/registry'
import { fileApi, type FileChange } from '../../services/fileApi'
import { renameEditorPaths } from '../../services/workspace/layout'
import { presetLabel, shellLabel, t } from '../../i18n'

const props = defineProps<{ tabsTarget: string; project: Project; settings: Settings; active: boolean; registry: TerminalRegistry; editors: EditorRegistry; shortcutsBlocked: boolean }>()
const emit = defineEmits<{ error: [message: string]; filesChanged: [projectId: string, change: FileChange]; activity: [projectId: string, state: Activity] }>()
const quickOpen = ref(false)
const openDirection = ref<Direction | undefined>()
let watchChannel: Awaited<ReturnType<typeof fileApi.watch>> | undefined
const layout = ref<Layout>(emptyLayout())
const configs = new Map<string, TerminalConfig>()
const shells = ref<ShellInfo[]>([])
const presets = ref<LaunchPreset[]>([])
const git = useGit()
const shell = ref<ShellType>(props.settings.default_shell)
watch(() => props.settings.default_shell, (value) => { shell.value = value })
const busy = ref(false)
const ready = ref(false)
const failed = ref(false)
const focused = ref<string | null>(null)
const activeTab = computed(() => layout.value.tabs.find((t) => t.id === layout.value.activeTabId))
const addMenu = computed<DropdownOption[]>(() => [
  ...shells.value.map((s) => ({ label: shellLabel(s), key: s.shellType, disabled: !s.available })),
  { type: 'divider', key: 'ai-divider' },
  { type: 'group', key: 'ai', label: t('workspace.aiCli'), children: presets.value.map((p) => ({ key: p.preset, label: presetLabel(p), disabled: !p.available || !shells.value.some((s) => s.shellType === 'powershell' && s.available) })) },
  { type: 'divider', key: 'divider' },
  { key: 'open-file', label: t('workspace.openFileMenu', { hint: hint('quickOpen') }) },
  { key: 'open-file-horizontal', label: t('workspace.fileSplitHorizontal'), disabled: !activeTab.value || !focused.value },
  { key: 'open-file-vertical', label: t('workspace.fileSplitVertical'), disabled: !activeTab.value || !focused.value },
])
const rename = ref<{ id: string; title: string } | null>(null)
const renameInput = ref<{ focus: () => void; select: () => void } | null>(null)
const tabMenu = ref<{ id: string; x: number; y: number } | null>(null)
const tabMenuOptions = computed<DropdownOption[]>(() => [{ key: 'rename', label: t('workspace.rename') }])
watch(rename, (value) => { if (value) void nextTick(() => { renameInput.value?.focus(); renameInput.value?.select() }) })
const confirmation = ref<{ text: string; action: () => Promise<void>; dirtyIds: string[] } | null>(null)
const confirmSaving = ref(false)
let disposed = false
let ratioTimer: ReturnType<typeof setTimeout> | undefined
let saves: Promise<void> = Promise.resolve()
let draggingTab: string | null = null

function error(reason: unknown) { emit('error', String(reason)) }
function terminalWarning(entry: TerminalEntry, message: string) {
  const config = Array.from(configs.values()).find((config) => props.registry.byConfig.get(config.id) === entry)
  showWorkspaceMessage(t('terminal.warning', { project: props.project.name, name: config?.name || config?.shellType || t('workspace.terminal'), message }), 'warning')
}
function save(next: Layout) {
  // 快照与保存顺序固定，较旧的比例保存不会覆盖新的结构变更。
  const snapshot = JSON.parse(JSON.stringify(next)) as Layout
  const operation = saves.catch(() => {}).then(() => workspaceApi.save(props.project.workspaceId, snapshot))
  saves = operation
  return operation
}
async function commit(next: Layout) {
  clearTimeout(ratioTimer)
  await save(next)
  layout.value = next
  await nextTick()
  refresh()
}
function refresh() {
  if (!props.active) return
  for (const leaf of activeTab.value ? leaves(activeTab.value.root) : []) {
    if (leaf.type === 'terminal') props.registry.byConfig.get(leaf.terminalConfigId)?.client.refreshSize()
  }
}
function restoreFocus() {
  if (disposed || !ready.value || !props.active || props.shortcutsBlocked || confirmation.value || rename.value || quickOpen.value || tabMenu.value) return
  if (Array.from(props.editors.entries.values()).some((entry) => entry.dialog)) return
  const panes = activeTab.value ? leaves(activeTab.value.root) : []
  const pane = panes.find((leaf) => (leaf.type === 'terminal' ? leaf.terminalConfigId : leaf.id) === focused.value) ?? panes[0]
  if (!pane) return
  focused.value = pane.type === 'terminal' ? pane.terminalConfigId : pane.id
  if (pane.type === 'terminal') props.registry.byConfig.get(pane.terminalConfigId)?.client.focus()
  else props.editors.entries.get(pane.id)?.view.focus()
}
function newEntry() {
  const entry = props.registry.create({ fontFamily: props.settings.font_family, fontSize: props.settings.font_size, scrollback: props.settings.scrollback }, error, (duration, exitCode) => {
    git.schedule(props.project.id)
    const threshold = props.settings.notify_command_seconds
    const tab = tabForEntry(entry)
    if (shouldNotify(threshold, duration, entry.visible, document.hasFocus())) void notify(entry.client.id ?? '', `${props.project.name} · ${tab?.title ?? t('workspace.terminal')}`, t('workspace.commandFinished', { code: exitCode ?? t('common.unknown'), seconds: Math.round(duration / 1000) }))
  }, (exitCode) => {
    // 修改项目路径或删除项目时后端先结束会话、视图稍后才卸载；延迟确认终端仍属于本视图。
    setTimeout(() => {
      if (disposed || entry.intentionalStop || !Array.from(props.registry.byConfig.values()).includes(entry)) return
      const tab = tabForEntry(entry)
      if (shouldNotify(props.settings.notify_command_seconds, null, entry.visible, document.hasFocus())) void notify(entry.client.id ?? '', `${props.project.name} · ${tab?.title ?? t('workspace.terminal')}`, t('workspace.terminalExited', { code: exitCode ?? t('common.unknown') }))
    }, 1000)
  }, (message) => terminalWarning(entry, message))
  return entry
}
function tabEntries(tab: WorkspaceTab) {
  return leaves(tab.root).flatMap((leaf) => { const entry = leaf.type === 'terminal' ? props.registry.byConfig.get(leaf.terminalConfigId) : undefined; return entry ? [entry] : [] })
}
function tabForEntry(entry: TerminalEntry) { return layout.value.tabs.find((tab) => tabEntries(tab).includes(entry)) }
function tabActivity(tab: WorkspaceTab) { return activity(tabEntries(tab)) }
watch(() => [props.active, layout.value], () => {
  for (const tab of layout.value.tabs) for (const entry of tabEntries(tab)) {
    entry.client.flushContentActivity()
    // v-show 隐藏已聚焦的终端时可能没有新的 focusin，主动撤销其窗口激活权限。
    if (entry.visible && !(props.active && tab.id === layout.value.activeTabId)) {
      entry.host.dispatchEvent(new Event('terminal-session-hidden', { bubbles: true }))
    }
    entry.visible = props.active && tab.id === layout.value.activeTabId
    if (entry.visible) entry.unread = false
  }
}, { flush: 'post' })
watch(() => activity(layout.value.tabs.flatMap(tabEntries)), (state) => emit('activity', props.project.id, state), { immediate: true })
async function exportOutput(id: string) {
  const entry = props.registry.byConfig.get(id)
  if (!entry) return
  try { await terminalApi.exportOutput(outputFilename(props.project.name, tabForEntry(entry)?.title ?? t('workspace.terminal')), exportBuffer(entry.client.term.buffer.active)) }
  catch (reason) { error(reason) }
}
async function start(id: string) {
  const entry = props.registry.byConfig.get(id)
  if (!entry || entry.status.state === 'running') return
  entry.status = { state: 'starting' }
  entry.intentionalStop = false
  try {
    const terminalId = await terminalApi.startTerminal(id, entry.client.measure() ?? INITIAL_SIZE, entry.client.createChannel())
    if (disposed) return
    entry.client.integration.reset()
    entry.shell = { integrated: false, cwd: null, command: 'idle', commandStartedAt: null, lastExitCode: null }
    entry.unread = false
    entry.client.reset()
    props.registry.bind(id, entry, terminalId)
  } catch (reason) {
    entry.status = { state: 'failed', message: String(reason) }
    error(reason)
  }
}
async function activate() {
  if (!ready.value || busy.value || !props.active) return
  busy.value = true
  try {
    // 已退出会话保留退出码，只自动启动本次运行尚无会话的配置。
    for (const id of configs.keys()) {
      if (disposed) return
      const entry = props.registry.byConfig.get(id)
      if (entry && !entry.client.id && entry.status.state === 'starting') await start(id)
    }
    await nextTick()
    refresh()
  } finally { busy.value = false }
}
async function load() {
  if (busy.value) return
  busy.value = true
  failed.value = false
  try {
    const next = await workspaceApi.load(props.project.workspaceId)
    const [stored, sessions, available, launchPresets] = await Promise.all([
      terminalApi.listTerminalConfigs(props.project.workspaceId), terminalApi.listTerminalSessions(), terminalApi.listAvailableShells(), terminalApi.listLaunchPresets(),
    ])
    if (disposed) return
    shells.value = available
    presets.value = launchPresets
    for (const config of stored) {
      configs.set(config.id, config)
      const entry = newEntry()
      props.registry.byConfig.set(config.id, entry)
      const session = sessions.find((s) => s.terminalConfigId === config.id)
      if (session) {
        entry.status = session.status
        terminalWarning(entry, t('workspace.historyLost'))
        try {
          await terminalApi.subscribeTerminal(session.terminalId, entry.client.createChannel())
          if (disposed) return
          props.registry.bind(config.id, entry, session.terminalId)
        } catch (reason) { entry.status = { state: 'failed', message: String(reason) }; error(reason) }
      }
    }
    layout.value = next
    for (const tab of next.tabs) for (const leaf of leaves(tab.root)) if (leaf.type === 'editor') props.editors.create(leaf.id, props.project.id, leaf.path, props.settings)
    try { watchChannel = await fileApi.watch(props.project.id, (change) => {
      if (disposed) return
      emit('filesChanged', props.project.id, change)
      void props.editors.external(props.project.id, 'paths' in change ? change.paths : undefined)
    }) } catch (reason) { error(t('workspace.watchFailed', { reason: String(reason) })) }
    focused.value = activeTab.value ? leaves(activeTab.value.root).map((leaf) => leaf.type === 'terminal' ? leaf.terminalConfigId : leaf.id)[0] ?? null : null
    ready.value = true
  } catch (reason) {
    failed.value = true
    for (const id of configs.keys()) props.registry.remove(id)
    configs.clear()
    error(reason)
  } finally { busy.value = false }
  await activate()
}
async function perform(action: () => Promise<void>) {
  if (busy.value || !ready.value) return
  busy.value = true
  try { await action() } catch (reason) { error(reason) } finally { busy.value = false }
}
async function create(direction?: Direction, shellType: ShellType = shell.value, launchPreset: TerminalConfig['launchPreset'] = null) {
  if (direction && (!activeTab.value || !focused.value)) return
  await perform(async () => {
    const entry = newEntry()
    const name = presets.value.find((p) => p.preset === launchPreset)?.description ?? shellType
    let id: string | undefined
    try {
      const created = await terminalApi.createTerminal(props.project.id, props.project.workspaceId, shellType, '', INITIAL_SIZE, entry.client.createChannel(), launchPreset)
      id = created.terminalConfigId
      props.registry.bind(id, entry, created.terminalId)
      const leaf = { type: 'terminal' as const, terminalConfigId: id }
      const next: Layout = direction && activeTab.value
        ? { ...layout.value, tabs: layout.value.tabs.map((tab) => tab.id === activeTab.value?.id ? { ...tab, root: splitLeaf(tab.root, focused.value!, leaf, direction) } : tab) }
        : (() => { const tab = { id: crypto.randomUUID(), title: name, root: leaf }; return { ...layout.value, activeTabId: tab.id, tabs: [...layout.value.tabs, tab] } })()
      if (disposed) throw new Error(t('workspace.viewClosed'))
      configs.set(id, { id, workspaceId: props.project.workspaceId, name, shellType, cwd: '', launchPreset })
      await commit(next)
      shell.value = shellType
      focused.value = id
      entry.client.focus()
    } catch (reason) {
      if (id) {
        try { await terminalApi.removeTerminal(id) } catch (cleanup) { error(t('workspace.cleanupFailed', { reason: String(cleanup) })) }
        props.registry.remove(id)
        configs.delete(id)
      } else entry.client.dispose()
      throw reason
    }
  })
}
async function confirmIfRunning(ids: string[], text: string, action: () => Promise<void>, confirmTerminal = true) {
  if (busy.value || confirmation.value) return
  let dirtyIds = ids.filter((id) => props.editors.entries.get(id)?.document.dirty)
  const entries = ids.flatMap((id) => { const entry = props.registry.byConfig.get(id); return entry && ['running', 'starting'].includes(entry.status.state) ? [entry] : [] })
  let requiresConfirmation = dirtyIds.length > 0 || (confirmTerminal && entries.length > 0)
  if (!requiresConfirmation && entries.length) {
    busy.value = true
    try {
      // 启动尚未完成时不跳过确认；运行中的智能体通过真实子进程识别。
      requiresConfirmation = entries.some((entry) => entry.status.state === 'starting' || !entry.client.id)
        || await terminalApi.terminalsHaveRunningTasks(entries.map((entry) => entry.client.id!))
    } catch (reason) {
      error(t('workspace.taskCheckFailed', { reason: String(reason) }))
      requiresConfirmation = true
    } finally { busy.value = false }
  }
  if (disposed) return
  // 等待任务查询时编辑器仍可输入，关闭前必须重新读取未保存状态。
  dirtyIds = ids.filter((id) => props.editors.entries.get(id)?.document.dirty)
  if (requiresConfirmation || dirtyIds.length) confirmation.value = { text: dirtyIds.length ? t('workspace.unsavedSuffix', { text, count: dirtyIds.length }) : text, action, dirtyIds }
  else await perform(action)
}
async function closeIds(ids: string[], tab?: WorkspaceTab) {
  // 待保存的比例布局仍引用即将删除的配置，必须在移除前取消。
  clearTimeout(ratioTimer)
  let next = layout.value
  let failure: unknown
  for (const id of ids) {
    try {
      if (props.editors.entries.has(id)) props.editors.remove(id)
      else { const entry = props.registry.byConfig.get(id); if (entry) entry.intentionalStop = true; try { await terminalApi.removeTerminal(id) } catch (reason) { if (entry) entry.intentionalStop = false; throw reason }; props.registry.remove(id); configs.delete(id) }
      next = removeFromLayout(next, id)
    } catch (reason) { failure = reason; break }
  }
  if (tab && !failure) {
    const tabs = next.tabs.filter((t) => t.id !== tab.id)
    next = { ...next, tabs, activeTabId: tabs.some((t) => t.id === next.activeTabId) ? next.activeTabId : tabs[0]?.id ?? null }
  }
  // 移除已完成的窗格即使后续移除失败，也必须更新界面。
  layout.value = next
  focused.value = activeTab.value ? leaves(activeTab.value.root).map((leaf) => leaf.type === 'terminal' ? leaf.terminalConfigId : leaf.id)[0] ?? null : null
  try { await commit(next) } catch (reason) {
    if (!failure) throw reason
    error(reason)
  }
  if (failure) throw failure
}
function closePane(id: string) {
  return confirmIfRunning([id], props.editors.entries.has(id) ? t('workspace.closeEditor') : t('workspace.closePane'), () => closeIds([id]), props.settings.confirm_close_terminal)
}
function closeTab(tab: WorkspaceTab) {
  const ids = leaves(tab.root).map((leaf) => leaf.type === 'terminal' ? leaf.terminalConfigId : leaf.id)
  return confirmIfRunning(ids, t('workspace.closeTab', { title: tab.title }), () => closeIds(ids, tab), props.settings.confirm_close_terminal)
}
function stop(id: string) {
  confirmIfRunning([id], t('workspace.stopTerminal'), async () => {
    const entry = props.registry.byConfig.get(id)
    if (entry?.client.id) {
      entry.intentionalStop = true
      try { await terminalApi.stopTerminal(entry.client.id) }
      catch (reason) { entry.intentionalStop = false; throw reason }
    }
  })
}
async function selectTab(id: string) {
  await perform(async () => {
    await commit({ ...layout.value, activeTabId: id })
    const leaf = activeTab.value ? leaves(activeTab.value.root)[0] : undefined
    focused.value = leaf?.type === 'terminal' ? leaf.terminalConfigId : leaf?.id ?? null
    if (focused.value) props.registry.byConfig.get(focused.value)?.client.focus()
  })
}
function ratio(tabId: string, path: number[], value: number) {
  if (busy.value) return
  layout.value = { ...layout.value, tabs: layout.value.tabs.map((tab) => tab.id === tabId ? { ...tab, root: setRatio(tab.root, path, value) } : tab) }
  clearTimeout(ratioTimer)
  ratioTimer = setTimeout(() => { void save(layout.value).catch(error) }, 500)
}
function dropTab(before: string) {
  const id = draggingTab
  draggingTab = null
  if (id) void perform(() => commit(moveTab(layout.value, id, before)))
}
async function confirmAction(saveFiles = false) {
  const pending = confirmation.value
  if (!pending || confirmSaving.value) return
  if (saveFiles) {
    confirmSaving.value = true
    try {
      for (const id of pending.dirtyIds) { await props.editors.save(id); if (props.editors.entries.get(id)?.document.dirty) { error(t('workspace.saveIncomplete')); return } }
    } finally { confirmSaving.value = false }
  }
  confirmation.value = null
  void perform(pending.action)
}
function chooseFile(direction?: Direction) { openDirection.value = direction; quickOpen.value = true }
function onAddSelect(key: string) {
  if (busy.value || !ready.value) return
  if (key === 'codex' || key === 'claude-code') { if (presets.value.some((p) => p.preset === key && p.available)) void create(undefined, 'powershell', key) }
  else if (key === 'open-file') chooseFile()
  else if (key === 'open-file-horizontal' || key === 'open-file-vertical') {
    if (activeTab.value && focused.value) chooseFile(key === 'open-file-horizontal' ? 'horizontal' : 'vertical')
  } else {
    const selected = shells.value.find((s) => s.shellType === key && s.available)
    if (selected) void create(undefined, selected.shellType)
  }
}
async function openFile(path: string, direction?: Direction) {
  await perform(async () => {
    const existing = layout.value.tabs.flatMap((tab) => leaves(tab.root).filter((leaf) => leaf.type === 'editor' && leaf.path.toLowerCase() === path.toLowerCase()).map((leaf) => ({ tab, leaf }))).at(0)
    if (existing && existing.leaf.type === 'editor') { await commit({ ...layout.value, activeTabId: existing.tab.id }); focused.value = existing.leaf.id; props.editors.entries.get(existing.leaf.id)?.view.focus(); return }
    const id = crypto.randomUUID()
    const leaf = { type: 'editor' as const, id, path }
    const next: Layout = direction && activeTab.value && focused.value
      ? { ...layout.value, tabs: layout.value.tabs.map((tab) => tab.id === activeTab.value?.id ? { ...tab, root: splitLeaf(tab.root, focused.value!, leaf, direction) } : tab) }
      : (() => { const tab = { id: crypto.randomUUID(), title: path.split('/').pop() ?? path, root: leaf }; return { ...layout.value, activeTabId: tab.id, tabs: [...layout.value.tabs, tab] } })()
    props.editors.create(id, props.project.id, path, props.settings)
    try { await commit(next); focused.value = id; props.editors.entries.get(id)?.view.focus() }
    catch (reason) { props.editors.remove(id); throw reason }
  })
}
async function renamed(from: string, to: string) {
  props.editors.rename(props.project.id, from, to)
  await perform(async () => {
    const next = renameEditorPaths(layout.value, from, to)
    // 磁盘重命名已完成，即使布局保存失败也展示新路径。
    layout.value = next
    await commit(next)
  })
}
defineExpose({ openFile, renamed, busy })
function saveRename() {
  const target = rename.value
  if (!target?.title.trim() || busy.value || !ready.value) return
  void perform(async () => {
    await commit({ ...layout.value, tabs: layout.value.tabs.map((tab) => tab.id === target.id ? { ...tab, title: target.title.trim() } : tab) })
    rename.value = null
  })
}
function openTabMenu(event: MouseEvent, tab: WorkspaceTab) {
  tabMenu.value = { id: tab.id, x: event.clientX, y: event.clientY }
}
function onTabMenuSelect(key: string | number) {
  const id = tabMenu.value?.id
  tabMenu.value = null
  if (key !== 'rename' || !id) return
  const tab = layout.value.tabs.find((item) => item.id === id)
  if (tab) rename.value = { id: tab.id, title: tab.title }
}
const hint = (id: string) => shortcutHint(id, props.settings.shortcuts)
function cycleTab(delta: number) {
  const tabs = layout.value.tabs
  if (tabs.length < 2) return
  const index = tabs.findIndex((tab) => tab.id === layout.value.activeTabId)
  const tab = tabs[(index + delta + tabs.length) % tabs.length]
  if (tab) void selectTab(tab.id)
}
function keydown(event: KeyboardEvent) {
  if (!props.active) return
  if (tabMenu.value && event.key === 'Escape' && !event.ctrlKey && !event.altKey && !event.metaKey) {
    tabMenu.value = null
    event.preventDefault()
    event.stopPropagation()
    return
  }
  if (props.shortcutsBlocked || confirmation.value || rename.value || quickOpen.value) return
  const command = matchShortcut(event, props.settings.shortcuts, ['workspace'])
  if (!command) return
  event.preventDefault()
  event.stopPropagation()
  if (event.repeat) return
  if (command === 'previousTab' || command === 'nextTab') cycleTab(command === 'previousTab' ? -1 : 1)
  else if (/^tab[1-9]$/.test(command)) {
    const tab = layout.value.tabs[Number(command.slice(3)) - 1]
    if (tab && tab.id !== layout.value.activeTabId) void selectTab(tab.id)
  } else if (command === 'newTerminal') void create()
  else if (command === 'quickOpen') chooseFile()
  else if (command === 'closePane' && focused.value) closePane(focused.value)
  else if (command === 'closeTab' && activeTab.value) closeTab(activeTab.value)
  else if (command === 'splitHorizontal') void create('horizontal')
  else if (command === 'splitVertical') void create('vertical')
}
watch(() => props.active, (active) => { if (!active) tabMenu.value = null; void activate() }, { flush: 'post' })
watch(() => [props.active, ready.value], async () => {
  if (!props.active || !ready.value) return
  // 等待窗格挂载及可见性更新，再恢复输入焦点；异步返回后重新检查激活状态。
  await nextTick()
  restoreFocus()
}, { flush: 'post' })
watch(() => props.settings.show_workspace_header, async () => { await nextTick(); refresh() })
onMounted(() => { document.addEventListener('keydown', keydown, true); void load() })
onBeforeUnmount(() => {
  disposed = true
  clearTimeout(ratioTimer)
  document.removeEventListener('keydown', keydown, true)
  for (const id of configs.keys()) props.registry.remove(id)
  if (watchChannel) watchChannel.onmessage = () => {}
  for (const entry of props.editors.entries.values()) if (entry.projectId === props.project.id) props.editors.remove(entry.id)
})
</script>

<template>
  <section v-show="active" class="project-workspace">
    <div v-if="settings.show_workspace_header" class="workspace-toolbar">
      <NButton :disabled="busy || !ready" :title="hint('quickOpen')" @click="chooseFile()">{{ t('workspace.openFile') }}</NButton>
      <NButton :disabled="busy || !activeTab || !focused" @click="chooseFile('horizontal')">{{ t('workspace.fileSplitHorizontalButton') }}</NButton>
      <NButton :disabled="busy || !activeTab || !focused" @click="chooseFile('vertical')">{{ t('workspace.fileSplitVerticalButton') }}</NButton>
      <NButton :disabled="busy || !activeTab || !focused" :title="hint('splitHorizontal')" @click="create('horizontal')">{{ t('workspace.splitHorizontal') }}</NButton>
      <NButton :disabled="busy || !activeTab || !focused" :title="hint('splitVertical')" @click="create('vertical')">{{ t('workspace.splitVertical') }}</NButton>
    </div>
    <Teleport v-if="active" :to="tabsTarget">
    <div class="workspace-tabs" role="tablist" :aria-label="t('workspace.tabs')">
      <div v-for="(tab, index) in layout.tabs" :key="tab.id" class="workspace-tab" :class="{ active: tab.id === layout.activeTabId }" :draggable="!busy" @dragstart="draggingTab = tab.id" @dragend="draggingTab = null" @dragover.prevent @drop.prevent="dropTab(tab.id)" @contextmenu.prevent.stop="openTabMenu($event, tab)">
        <button role="tab" :aria-selected="tab.id === layout.activeTabId" :title="index < 9 ? hint(`tab${index + 1}`) : undefined" @click="selectTab(tab.id)" @dblclick="rename = { id: tab.id, title: tab.title }"><span v-if="tabActivity(tab)" class="activity-dot" :class="tabActivity(tab)" :title="tabActivity(tab) === 'failed' ? t('workspace.activityFailed') : t('workspace.activityUnread')" />{{ tab.title }}{{ leaves(tab.root).some((leaf) => leaf.type === 'editor' && editors.entries.get(leaf.id)?.document.dirty) ? ' ●' : '' }}</button>
        <button :disabled="busy" :aria-label="t('workspace.closeTabButton', { title: tab.title })" @click="closeTab(tab)">×</button>
      </div>
      <NDropdown trigger="click" :options="addMenu" :disabled="busy || !ready" @select="onAddSelect">
        <button class="workspace-tab-add" :disabled="busy || !ready" :aria-label="t('workspace.new')" :title="t('workspace.newTitle', { hint: hint('newTerminal') })"><UiIcon name="plus" :size="16" /></button>
      </NDropdown>
    </div>
    </Teleport>
    <p v-if="!ready" class="workspace-message">{{ failed ? t('workspace.loadFailed') : t('workspace.restoring') }} <NButton v-if="failed" @click="load">{{ t('common.retry') }}</NButton></p>
    <div v-else-if="!layout.tabs.length" class="workspace-message"><UiIcon name="terminal" :size="32" /><h2>{{ t('workspace.emptyTitle') }}</h2><p>{{ t('workspace.emptyBody') }}</p><NButton :disabled="busy" @click="create()">{{ t('workspace.newTerminal') }}</NButton><p>{{ t('workspace.emptyHints', { terminal: hint('newTerminal'), file: hint('quickOpen') }) }}</p></div>
    <div v-for="tab in layout.tabs" v-show="tab.id === layout.activeTabId" :key="tab.id" class="tab-content">
      <LayoutPane :node="tab.root" :path="[]" :registry="registry" :editors="editors" :configs="configs" :focused="focused" :visible="active && tab.id === layout.activeTabId" @focus="focused = $event" @close="closePane" @restart="(id) => perform(() => start(id))" @stop="stop" @export="exportOutput" @ratio="(path, value) => ratio(tab.id, path, value)" />
    </div>
    <QuickOpen v-model:show="quickOpen" :project-id="project.id" @open="(path) => openFile(path, openDirection)" @error="error" />
    <NModal :show="!!confirmation" preset="card" :title="t('workspace.confirmClose')" class="form-modal" @update:show="(show) => { if (!show && !confirmSaving) confirmation = null }">
      <p>{{ confirmation?.text }}</p>
      <div class="dialog-actions"><NButton :disabled="confirmSaving" @click="confirmation = null">{{ t('common.cancel') }}</NButton><NButton v-if="confirmation?.dirtyIds.length" :loading="confirmSaving" @click="confirmAction(true)">{{ t('workspace.saveAndClose') }}</NButton><NButton type="error" :disabled="confirmSaving" @click="confirmAction(false)">{{ confirmation?.dirtyIds.length ? t('workspace.discardAndClose') : t('common.confirm') }}</NButton></div>
    </NModal>
    <NDropdown trigger="manual" placement="bottom-start" :show="!!tabMenu" :x="tabMenu?.x ?? 0" :y="tabMenu?.y ?? 0" :options="tabMenuOptions" @select="onTabMenuSelect" @clickoutside="tabMenu = null" />
    <NModal :show="!!rename" preset="card" :title="t('workspace.rename')" class="form-modal" @update:show="(show) => { if (!show) rename = null }">
      <NInput v-if="rename" ref="renameInput" v-model:value="rename.title" :maxlength="100" :placeholder="t('workspace.tabTitle')" @keydown.enter.prevent="saveRename" />
      <div class="dialog-actions"><NButton @click="rename = null">{{ t('common.cancel') }}</NButton><NButton :disabled="busy || !rename?.title.trim()" @click="saveRename">{{ t('common.save') }}</NButton></div>
    </NModal>
  </section>
</template>
