<!-- Author: jaswtao · Date: 2026-10-12 -->
<script setup lang="ts">
import { createRenameTargetTracker } from './services/workspace/renameTarget'
import { matchShortcut } from './services/workspace/shortcuts'
import { filterProjects } from './services/sidebar/filter'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch, watchEffect } from 'vue'
import {
  NNotificationProvider,
  NButton,
  NConfigProvider,
  darkTheme,
  dateEnUS,
  dateZhCN,
  enUS,
  zhCN,
  useOsTheme,
} from 'naive-ui'
import { locale, setLocale, t } from './i18n'
import {
  type Project,
  type ProjectGroup,
} from './services/projectApi'
import { useProjects } from './stores/projects'
import WorkspaceDeck from './components/workspace/WorkspaceDeck.vue'
import FileTree from './components/files/FileTree.vue'
import type { FileChange } from './services/fileApi'
import SettingsDialog from './components/SettingsDialog.vue'
import TitleBar from './components/TitleBar.vue'
import UiIcon from './components/UiIcon.vue'
import WorkspaceMessages from './components/WorkspaceMessages.vue'
import { showWorkspaceMessage } from './services/workspace/messages'
import { appearanceVariables, componentTheme, resolveAppearance } from './services/appearance'
import { useGit } from './stores/git'
import type { Activity } from './services/workspace/activity'
import WorktreeDialog from './components/dialogs/WorktreeDialog.vue'
import { lifecycleApi } from './services/lifecycleApi'
import ProjectSidebar from './components/sidebar/ProjectSidebar.vue'
import ProjectRenameDialog from './components/dialogs/ProjectRenameDialog.vue'
import ProjectDialog from './components/dialogs/ProjectDialog.vue'
import GroupDialog from './components/dialogs/GroupDialog.vue'
import DeleteConfirmDialog from './components/dialogs/DeleteConfirmDialog.vue'
import CloseConfirmDialog from './components/dialogs/CloseConfirmDialog.vue'
import { bindTerminalActivation } from './services/terminal/activation'
import { terminalApi } from './services/terminalApi'
const unbindTerminalActivation = bindTerminalActivation(document, terminalApi.setActiveTerminal)
onBeforeUnmount(unbindTerminalActivation)
const git = useGit()
const activities = reactive(new Map<string, Activity>())
const worktreeDialog = ref(false)
const worktreeProject = ref<Project | null>(null)
function openWorktree(project: Project) { worktreeProject.value = project; worktreeDialog.value = true }
async function windowFocus() {
  if (store.active) git.schedule(store.active.id)
  await nextTick()
  if (!disposed && document.hasFocus()) deck.value?.focusTerminal()
}
const settingsDialog = ref(false)
const closeDialog = ref(false)
const closeBusy = ref(false)
const recoveryDialog = ref(false)
let closeRequestPending = false
let unlistenClose: (() => void) | undefined
let disposed = false
const deck = ref<InstanceType<typeof WorkspaceDeck>>()
const sidebarView = ref<'projects' | 'files'>('projects')
const sidebarVisible = ref(readSidebarVisible())
function readSidebarVisible(): boolean {
  try { return localStorage.getItem('orbyte.sidebar-visible') !== 'false' }
  catch { return true }
}
function setSidebarVisible(visible: boolean) {
  sidebarVisible.value = visible
  // 本地存储不可用时，仍可在当前窗口中切换侧栏。
  try { localStorage.setItem('orbyte.sidebar-visible', String(visible)) } catch {}
}
const SIDEBAR_DEFAULT = 280
const SIDEBAR_MIN = 220
const SIDEBAR_MAX = 640
const sidebarWidth = ref(readSidebarWidth())
const sidebarResizing = ref(false)
let cleanupResize: (() => void) | undefined
function clampSidebarWidth(width: number) {
  const max = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, window.innerWidth - 280))
  return Math.min(max, Math.max(SIDEBAR_MIN, Math.round(width)))
}
function readSidebarWidth() {
  try {
    const stored = Number(localStorage.getItem('orbyte.sidebar-width'))
    if (Number.isFinite(stored) && stored > 0) return clampSidebarWidth(stored)
  } catch { /* 本地存储不可用时使用默认宽度。 */ }
  return SIDEBAR_DEFAULT
}
function persistSidebarWidth() {
  try { localStorage.setItem('orbyte.sidebar-width', String(sidebarWidth.value)) } catch { /* 宽度仍在当前窗口生效。 */ }
}
function startSidebarResize(event: PointerEvent) {
  if (event.button !== 0) return
  event.preventDefault()
  cleanupResize?.()
  sidebarResizing.value = true
  const startX = event.clientX
  const startWidth = sidebarWidth.value
  const move = (e: PointerEvent) => { sidebarWidth.value = clampSidebarWidth(startWidth + e.clientX - startX) }
  const end = () => {
    document.removeEventListener('pointermove', move)
    document.removeEventListener('pointerup', end)
    document.removeEventListener('pointercancel', end)
    sidebarResizing.value = false
    persistSidebarWidth()
    cleanupResize = undefined
  }
  cleanupResize = end
  document.addEventListener('pointermove', move)
  document.addEventListener('pointerup', end)
  document.addEventListener('pointercancel', end)
}
function onSidebarResizeKey(event: KeyboardEvent) {
  const delta = event.key === 'ArrowLeft' ? -16 : event.key === 'ArrowRight' ? 16 : 0
  if (!delta) return
  event.preventDefault()
  sidebarWidth.value = clampSidebarWidth(sidebarWidth.value + delta)
  persistSidebarWidth()
}
const fileChanges = reactive(new Map<string, FileChange>())

const store = useProjects()
watch(() => store.error, (message) => { if (message) showWorkspaceMessage(message) }, { flush: 'sync' })
function reportError(message: string) {
  // 错误状态可能连续相同，但每次失败仍应重新显示通知。
  if (store.error === message) showWorkspaceMessage(message)
  else store.error = message
}
watch(() => store.active?.id, (id) => { if (id) git.activate(id) })
const osTheme = useOsTheme()
const naiveLocale = computed(() => locale.value === 'zh' ? zhCN : enUS)
const naiveDateLocale = computed(() => locale.value === 'zh' ? dateZhCN : dateEnUS)
watch(() => store.settings?.language, (value) => setLocale(value === 'zh' ? 'zh' : 'en'), { immediate: true })
watch(locale, (value) => { document.documentElement.lang = value }, { immediate: true })
const appearance = computed(() => resolveAppearance(store.settings?.theme, osTheme.value))
const theme = computed(() => appearance.value === 'dark' ? darkTheme : null)
const themeOverrides = computed(() => componentTheme(appearance.value))
watchEffect(() => {
  // 根节点上的变量同时覆盖 Teleport 到 body 的弹窗和菜单。
  for (const [name, value] of Object.entries(appearanceVariables(appearance.value, store.settings?.workspace_border_color, store.settings?.workspace_border_intensity))) document.documentElement.style.setProperty(name, value)
  document.documentElement.style.colorScheme = appearance.value
})
const sidebar = ref<InstanceType<typeof ProjectSidebar>>()
const projectDialog = ref(false)
const renamingProject = ref<Project | null>(null)
const renameTarget = createRenameTargetTracker()
const editingProject = ref<Project | null>(null)
const defaultGroupId = ref('')
const groupDialog = ref(false)
const editingGroup = ref<ProjectGroup | null>(null)
const deleting = ref<{ kind: 'project' | 'group'; id: string; name: string } | null>(null)
function editProject(project?: Project, groupId?: string) { editingProject.value = project ?? null; defaultGroupId.value = project ? '' : (groupId ?? ''); projectDialog.value = true }
function editGroup(group?: ProjectGroup) { editingGroup.value = group ?? null; groupDialog.value = true }
function confirmProjectChange(id: string) { return deck.value?.confirmProjectChange(id) ?? Promise.resolve(true) }
async function workspaceShortcut(event: KeyboardEvent) {
  if (renamingProject.value || projectDialog.value || groupDialog.value || deleting.value || settingsDialog.value || closeDialog.value || worktreeDialog.value || recoveryDialog.value) return
  const command = matchShortcut(event, store.settings?.shortcuts, ['project'])
  if (!command || command === 'rename') return
  event.preventDefault()
  event.stopPropagation()
  if (command === 'projectSearch') {
    setSidebarVisible(true)
    sidebarView.value = 'projects'
    void nextTick(() => sidebar.value?.focusSearch())
  } else if (command === 'sidebarNarrow' || command === 'sidebarWiden') {
    sidebarWidth.value = clampSidebarWidth(sidebarWidth.value + (command === 'sidebarNarrow' ? -16 : 16))
    persistSidebarWidth()
  } else if (['projectPrevious', 'projectNext', 'projectUp', 'projectDown'].includes(command)) {
    if (event.repeat || store.busy || !store.projects.length) return
    const vertical = command === 'projectUp' || command === 'projectDown'
    if (vertical) {
      setSidebarVisible(true)
      sidebarView.value = 'projects'
      await nextTick()
      if (disposed || store.busy) return
    }
    const projects = vertical
      ? sidebar.value?.getNavigationProjects() ?? [...store.groups.map((group) => group.id), ''].flatMap((id) => filterProjects(store.projects, store.settings?.show_hidden_projects ?? false).filter((project) => (project.groupId ?? '') === id))
      : filterProjects(store.projects, store.settings?.show_hidden_projects ?? false)
    if (!projects.length) return
    const index = projects.findIndex((project) => project.id === store.active?.id)
    const delta = command === 'projectPrevious' || command === 'projectUp' ? -1 : 1
    const project = projects[index < 0 ? (delta < 0 ? projects.length - 1 : 0) : (index + delta + projects.length) % projects.length]
    if (!project) return
    if (!vertical) {
      void store.activate(project.id)
      return
    }
    if (await store.activate(project.id)) {
      await nextTick()
      if (!disposed && store.active?.id === project.id) sidebar.value?.revealProject(project.id)
    }
  }
}
function renameShortcut(event: KeyboardEvent) {
  const command = matchShortcut(event, store.settings?.shortcuts, ['project'])
  renameTarget.keydown(event, command === 'rename')
  if (command !== 'rename' || event.repeat || disposed || !store.ready || store.busy) return
  if (renamingProject.value || projectDialog.value || groupDialog.value || deleting.value || settingsDialog.value || closeDialog.value || worktreeDialog.value || recoveryDialog.value) return
  if (Array.from(document.querySelectorAll('[aria-modal="true"], .n-dropdown-menu')).some((element) => element.getClientRects().length)) return
  const target = renameTarget.target
  if (!target) return
  if (target.kind === 'project') {
    const project = store.projects.find((item) => item.id === target.id)
    if (!project || !sidebarVisible.value || sidebarView.value !== 'projects') return
    renamingProject.value = project
  } else if (!deck.value?.renameActiveTab()) return
  event.preventDefault()
  event.stopImmediatePropagation()
}
async function closeShortcut(event: KeyboardEvent) {
  if (event.code !== 'F4' || !event.altKey || event.ctrlKey || event.shiftKey || event.metaKey) return
  event.preventDefault()
  event.stopImmediatePropagation()
  if (event.repeat || event.isComposing || disposed || closeRequestPending || closeDialog.value || closeBusy.value) return
  closeRequestPending = true
  try { await getCurrentWindow().close() }
  catch (reason) { reportError(t('titlebar.windowActionFailed', { reason: String(reason) })) }
  finally { closeRequestPending = false }
}
// Child onMounted runs before the parent hook, so register here to capture Alt+F4 first.
document.addEventListener('keydown', closeShortcut, true)
document.addEventListener('keydown', renameShortcut, true)
document.addEventListener('pointerdown', renameTarget.pointerdown, true)
document.addEventListener('focusin', renameTarget.focusin, true)
window.addEventListener('blur', renameTarget.reset)
onBeforeUnmount(() => {
  document.removeEventListener('keydown', renameShortcut, true)
  document.removeEventListener('pointerdown', renameTarget.pointerdown, true)
  document.removeEventListener('focusin', renameTarget.focusin, true)
  window.removeEventListener('blur', renameTarget.reset)
})
async function resolveClose(action: 'save' | 'discard' | 'cancel') {
  if (closeBusy.value) return
  closeBusy.value = true
  try {
    if (action === 'save' && (!deck.value || !await deck.value.saveAllEditors())) {
      reportError(t('app.unsavedOnExit'))
      await lifecycleApi.resolveClose(false)
      closeDialog.value = false
      return
    }
    if (action === 'discard') await deck.value?.discardAllEditors()
    await lifecycleApi.resolveClose(action !== 'cancel')
    closeDialog.value = false
  } catch (reason) { reportError(String(reason)) }
  finally { closeBusy.value = false }
}
onMounted(async () => {
  document.addEventListener('keydown', workspaceShortcut, true)
  window.addEventListener('focus', windowFocus)
  void git.init()
  try {
    unlistenClose = await lifecycleApi.onCloseRequested(() => { closeDialog.value = true })
    if (disposed) { unlistenClose(); return }
    await store.run(async () => {})
  } catch (reason) { reportError(t('app.closeInitFailed', { reason: String(reason) })) }
})
onBeforeUnmount(() => { disposed = true; cleanupResize?.(); unlistenClose?.(); document.removeEventListener('keydown', closeShortcut, true); document.removeEventListener('keydown', workspaceShortcut, true); window.removeEventListener('focus', windowFocus) })
</script>

<template>
  <NConfigProvider :theme="theme" :theme-overrides="themeOverrides" :locale="naiveLocale" :date-locale="naiveDateLocale">
    <NNotificationProvider placement="top-right" :max="3" container-class="workspace-notifications">
    <WorkspaceMessages />
    <div class="workbench" :class="{ light: !theme, 'sidebar-resizing': sidebarResizing }" :style="{ '--sidebar-width': `${sidebarWidth}px` }">
      <TitleBar :sidebar-visible="sidebarVisible" :settings-disabled="!store.ready || store.busy || closeDialog"
        @toggle-sidebar="setSidebarVisible(!sidebarVisible)" @settings="settingsDialog = true" @error="reportError">
        <div id="workspace-titlebar-tabs" data-tauri-drag-region />
      </TitleBar>
      <div class="body">
        <aside id="workbench-sidebar" v-show="sidebarVisible" class="sidebar">
          <div class="sidebar-segments" role="group" :aria-label="t('app.sidebarView')">
            <button :class="{ active: sidebarView === 'projects' }" :aria-pressed="sidebarView === 'projects'" @click="sidebarView = 'projects'">{{ t('app.projects') }}</button>
            <button :class="{ active: sidebarView === 'files' }" :aria-pressed="sidebarView === 'files'" :disabled="!store.active" @click="sidebarView = 'files'">{{ t('app.files') }}</button>
          </div>
          <FileTree v-if="sidebarView === 'files' && store.active" :key="`${store.active.id}:${store.active.path}`"
            :project-id="store.active.id" :disabled="deck?.mutationBusy ?? true" :change="fileChanges.get(store.active.id) ?? null"
            @open="deck?.openFile($event)" @rename="(from, to) => deck?.renamed(from, to)" @error="reportError" />
          <KeepAlive>
          <ProjectSidebar v-if="sidebarView !== 'files' || !store.active" ref="sidebar" :activities="activities" @worktree="openWorktree"
            @edit-project="editProject" @edit-group="editGroup" @delete-item="deleting = $event" />
          </KeepAlive>
        </aside>
        <div v-show="sidebarVisible" class="sidebar-resizer" role="separator" aria-orientation="vertical" :aria-label="t('app.resizeSidebar')" :title="t('app.resizeSidebarTitle')" :aria-valuenow="sidebarWidth" :aria-valuemin="SIDEBAR_MIN" :aria-valuemax="SIDEBAR_MAX" tabindex="0" @pointerdown="startSidebarResize" @keydown="onSidebarResizeKey" />
        <main class="workspace">
          <header v-if="store.active && store.settings?.show_workspace_header" class="workspace-header">
            <div><h1>{{ store.active.name }}</h1><p>{{ store.active.path }}</p></div>
            <NButton :disabled="store.busy" @click="editProject(store.active)">{{ t('app.editProject') }}</NButton>
            <NButton :disabled="store.busy" @click="deleting = { kind: 'project', id: store.active.id, name: store.active.name }">{{ t('app.deleteProject') }}</NButton>
          </header>
          <div v-if="!store.active" class="workspace-placeholder">
            <div class="welcome-mark"><UiIcon name="terminal" :size="32" /></div>
            <div class="welcome-copy"><h1>{{ t('app.welcomeTitle') }}</h1><p>{{ store.ready ? t('app.welcomeReady') : store.error ? t('app.welcomeError') : t('app.welcomeLoading') }}</p></div>
            <span class="welcome-brand">{{ t('app.brand') }}</span>
            <NButton v-if="store.ready" type="primary" @click="editProject()">{{ t('app.addProject') }}</NButton>
            <NButton v-else-if="store.error" @click="store.run(async () => {})">{{ t('common.retry') }}</NButton>
          </div>
          <WorkspaceDeck @recovery-visibility="recoveryDialog = $event" @activate="store.activate($event)" ref="deck" v-if="store.ready && store.settings" v-show="!!store.active" tabs-target="#workspace-titlebar-tabs"
            :appearance="appearance" :projects="store.projects" :settings="store.settings" :active-id="store.active?.id ?? null"
            :shortcuts-blocked="!!renamingProject || recoveryDialog || worktreeDialog || projectDialog || groupDialog || !!deleting || settingsDialog || closeDialog"
            @error="reportError" @activity="(id, state) => activities.set(id, state)"
            @files-changed="(id, change) => { fileChanges.set(id, change); git.schedule(id) }" />
        </main>
      </div>
      <SettingsDialog v-model:show="settingsDialog" />
      <WorktreeDialog v-model:show="worktreeDialog" :project="worktreeProject" />
      <ProjectDialog v-model:show="projectDialog" :project="editingProject" :default-group-id="defaultGroupId" :confirm-project-change="confirmProjectChange" />
      <ProjectRenameDialog v-model:project="renamingProject" />
      <GroupDialog v-model:show="groupDialog" :group="editingGroup" />
      <DeleteConfirmDialog v-model:item="deleting" :confirm-project-change="confirmProjectChange" />
      <CloseConfirmDialog :show="closeDialog" :busy="closeBusy" :error="store.error" @resolve="resolveClose" />
    </div>
    </NNotificationProvider>
  </NConfigProvider>
</template>
