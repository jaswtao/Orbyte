<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { NButton, NInput, NSwitch } from 'naive-ui'
import { projectApi, type Project, type ProjectGroup } from '../../services/projectApi'
import { useProjects } from '../../stores/projects'
import { buildMenuOptions } from '../../services/sidebar/menu'
import { filterProjects, firstVisibleProject } from '../../services/sidebar/filter'
import { dragPayload, parseDragPayload, reorderGroupIds, reorderProjectEntries } from '../../services/sidebar/reorder'
import { useGit } from '../../stores/git'
import type { Activity } from '../../services/workspace/activity'
import ProjectContextMenu from './ProjectContextMenu.vue'
import UiIcon from '../UiIcon.vue'
import { t } from '../../i18n'
const store = useProjects()
const git = useGit()
defineProps<{ activities: Map<string, Activity> }>()
const emit = defineEmits<{ worktree: [project: Project]; editProject: [project?: Project, groupId?: string]; editGroup: [group?: ProjectGroup]; deleteItem: [item: { kind: 'project' | 'group'; id: string; name: string }] }>()
const search = ref('')
const searchInput = ref<{ focus: () => void } | null>(null)
const projectList = ref<HTMLElement | null>(null)
const favoritesOnly = ref(false)
const collapsed = reactive(new Set<string>())
const sections = computed(() => [
  ...store.groups,
  { id: '', name: t('sidebar.ungrouped'), sortOrder: 0 },
])
const filteredProjects = computed(() => filterProjects(store.projects, store.settings?.show_hidden_projects ?? false, favoritesOnly.value, search.value))
const visibleProjects = (id: string) => filteredProjects.value.filter((p) => (p.groupId ?? '') === id)

function replacementProject(excludedId: string) {
  return firstVisibleProject(store.projects, sections.value.map((group) => group.id), excludedId, favoritesOnly.value, search.value)
}
async function toggleHidden(project: Project) {
  const replacement = !project.hidden && store.active?.id === project.id ? replacementProject(project.id) : undefined
  if (await store.run(() => projectApi.setProjectHidden(project.id, !project.hidden, replacement?.id ?? null))) {
    if (replacement) collapsed.delete(replacement.groupId ?? '')
  }
}
async function setShowHidden(show: boolean) {
  const active = store.active
  const switchProject = !show && !!active?.hidden
  const replacement = switchProject ? replacementProject(active.id) : undefined
  if (await store.run(() => projectApi.setSettings({
    show_hidden_projects: show,
    ...(switchProject ? { active_project_id: replacement?.id ?? null } : {}),
  }))) {
    if (replacement) collapsed.delete(replacement.groupId ?? '')
  }
}
const dragging = ref<{ kind: 'project' | 'group'; id: string } | null>(null)
const dropHint = ref<{ groupId: string; beforeId: string | null } | null>(null)
let swallowClick = false
const contextMenu = ref<{ kind: 'group' | 'project'; id: string; x: number; y: number } | null>(null)
const menuOptions = computed(() => {
  const options = buildMenuOptions(contextMenu.value, store.projects, store.settings, store.busy || !store.ready)
  if (contextMenu.value?.kind === 'project' && git.available) options.push({ key: 'worktree', label: 'Worktree…', disabled: store.busy || !store.ready || !git.statuses.get(contextMenu.value.id)?.isRepo })
  return options
})
function openMenu(event: MouseEvent, kind: 'group' | 'project', id: string) {
  contextMenu.value = kind === 'group' && !id ? null : { kind, id, x: event.clientX, y: event.clientY }
}
function onMenuSelect(key: string) {
  const target = contextMenu.value
  contextMenu.value = null
  if (!target || store.busy || !store.ready) return
  if (target.kind === 'group') {
    const group = store.groups.find((g) => g.id === target.id)
    if (!group) return
    if (key === 'add-project') emit('editProject', undefined, group.id)
    else if (key === 'rename') emit('editGroup', group)
    else if (key === 'delete') emit('deleteItem', { kind: 'group', id: group.id, name: group.name })
  } else {
    const project = store.projects.find((p) => p.id === target.id)
    if (!project) return
    if (key === 'worktree') emit('worktree', project)
    else if (key === 'edit') emit('editProject', project)
    else if (key === 'favorite') void toggleFavorite(project)
    else if (key === 'hidden') void toggleHidden(project)
    else if (key === 'delete') emit('deleteItem', { kind: 'project', id: project.id, name: project.name })
    else if (key === 'toggle-header' && store.settings) {
      const show = !store.settings.show_workspace_header
      void store.run(() => projectApi.setSetting('show_workspace_header', show))
    }
  }
}

function consumeDragClick() {
  if (!swallowClick) return false
  swallowClick = false
  return true
}
function toggleCollapsed(id: string) {
  if (consumeDragClick()) return
  if (collapsed.has(id)) collapsed.delete(id)
  else collapsed.add(id)
}
function activateProject(id: string) {
  if (consumeDragClick()) return
  void store.activate(id)
}
function toggleFavorite(project: Project) {
  return store.run(() =>
    projectApi.updateProject(project.id, {
      ...project,
      favorite: !project.favorite,
    }),
  )
}
function startDrag(event: DragEvent, kind: 'project' | 'group', id: string) {
  if (store.busy || !id) {
    event.preventDefault()
    return
  }
  dragging.value = { kind, id }
  event.dataTransfer?.setData('text/plain', dragPayload(kind, id))
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}
function endDrag() {
  dropHint.value = null
  swallowClick = true
  window.setTimeout(() => { swallowClick = false }, 0)
  const source = dragging.value
  window.setTimeout(() => {
    if (dragging.value === source) dragging.value = null
  }, 0)
}
function dragSource(event: DragEvent) {
  return parseDragPayload(event.dataTransfer?.getData('text/plain') ?? '') ?? dragging.value
}
function allowDrop(event: DragEvent) {
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
}
function leaveDropTarget(event: DragEvent) {
  const target = event.currentTarget as HTMLElement
  // 子元素之间移动仍属于同一落点，不清理提示。
  if (event.relatedTarget instanceof Node && target.contains(event.relatedTarget)) return
  dropHint.value = null
}
function overGroup(event: DragEvent, groupId: string) {
  allowDrop(event)
  const source = dragging.value
  if (!source) return
  if (source.kind === 'group') {
    dropHint.value = groupId && source.id !== groupId ? { groupId, beforeId: null } : null
    return
  }
  dropHint.value = { groupId, beforeId: null }
}
function overProject(event: DragEvent, groupId: string, projectId: string) {
  allowDrop(event)
  const source = dragging.value
  if (!source || source.kind !== 'project' || source.id === projectId) {
    dropHint.value = null
    return
  }
  dropHint.value = { groupId, beforeId: projectId }
}
async function drop(event: DragEvent, group: string, before?: string) {
  const source = dragSource(event)
  dragging.value = null
  endDrag()
  if (!source || store.busy) return
  if (source.kind === 'group') {
    if (!group || before || source.id === group) return
    const ids = reorderGroupIds(store.groups.map((g) => g.id), source.id, group)
    await store.run(() => projectApi.reorderGroups(ids))
  } else {
    if (before === source.id) return
    const ordered = reorderProjectEntries(store.projects, source.id, group || null, before)
    const current = store.projects.map((project) => [project.id, project.groupId])
    if (ordered.every(([id, groupId], index) => current[index]?.[0] === id && current[index]?.[1] === groupId)) return
    await store.run(() => projectApi.reorderProjects(ordered))
  }
}
function revealProject(id: string) {
  const list = projectList.value
  const row = Array.from(list?.querySelectorAll<HTMLElement>('.project-row') ?? []).find((element) => element.dataset.projectId === id)
  if (!list || !row) return
  const listRect = list.getBoundingClientRect()
  const rowRect = row.getBoundingClientRect()
  const top = listRect.top + list.clientTop
  const bottom = top + list.clientHeight
  if (rowRect.top < top) list.scrollTop += rowRect.top - top
  else if (rowRect.bottom > bottom) list.scrollTop += rowRect.bottom - bottom
}
defineExpose({
  revealProject,
  focusSearch: () => searchInput.value?.focus(),
  getNavigationProjects: () => sections.value.flatMap((group) => collapsed.has(group.id) ? [] : visibleProjects(group.id)),
})
</script>
<template>
          <div class="sidebar-heading">
            <strong>{{ t('app.projects') }}</strong
            ><NButton
              size="small"
              :disabled="!store.ready || store.busy"
              @click="emit('editGroup')"
              >{{ t('sidebar.newGroup') }}</NButton
            ><NButton
              size="small"
              :disabled="!store.ready || store.busy"
              @click="emit('editProject')"
              >{{ t('sidebar.addProject') }}</NButton
            >
          </div>
          <NInput
            ref="searchInput"
            v-model:value="search"
            :placeholder="t('sidebar.searchPlaceholder')"
            clearable
            :aria-label="t('sidebar.search')"
          ><template #prefix><UiIcon name="search" :size="15" /></template></NInput>
          <div class="project-filters">
            <label class="favorites-filter"
              ><NSwitch v-model:value="favoritesOnly" size="small" />
              {{ t('sidebar.favoritesOnly') }}</label
            >
            <label class="favorites-filter">
              <NSwitch :value="store.settings?.show_hidden_projects ?? false" size="small" :disabled="!store.ready || store.busy" @update:value="setShowHidden" />
              {{ t('sidebar.showHiddenProjects') }}
            </label>
          </div>
          <div ref="projectList" class="project-list">
            <section
              v-for="group in sections"
              :key="group.id"
              class="group"
              :class="{ 'drop-target': dropHint?.groupId === group.id && !dropHint.beforeId }"
              @dragenter.prevent
              @dragover.prevent="overGroup($event, group.id)"
              @dragleave.stop="leaveDropTarget"
              @drop.prevent.stop="drop($event, group.id)"
            >
              <div
                class="group-heading"
                @contextmenu.prevent.stop="openMenu($event, 'group', group.id)"
                @dragenter.prevent.stop
                @dragover.prevent.stop="overGroup($event, group.id)"
                @dragleave.stop="leaveDropTarget"
                @drop.prevent.stop="drop($event, group.id)"
              >
                <button
                  class="group-toggle"
                  :aria-expanded="!collapsed.has(group.id)"
                  :draggable="!!group.id && !store.busy"
                  @click="toggleCollapsed(group.id)"
                  @dragstart.stop="startDrag($event, 'group', group.id)"
                  @dragend="endDrag"
                >
                  <UiIcon name="chevron" :size="12" /><span class="group-name">{{ group.name }}</span>
                  <small>{{ visibleProjects(group.id).length }}</small>
                </button>
              </div>
              <div v-show="!collapsed.has(group.id)" class="group-projects">
                <div
                  v-for="project in visibleProjects(group.id)"
                  :key="project.id"
                  class="project-row"
                  :data-project-id="project.id"
                  @contextmenu.prevent.stop="openMenu($event, 'project', project.id)"
                  :class="{
                    selected: store.active?.id === project.id,
                    dragging: dragging?.kind === 'project' && dragging.id === project.id,
                    'drop-before': dropHint?.beforeId === project.id,
                  }"
                  @dragenter.prevent.stop
                  @dragover.prevent.stop="overProject($event, group.id, project.id)"
                  @dragleave.stop="leaveDropTarget"
                  @drop.prevent.stop="drop($event, group.id, project.id)"
                >
                  <UiIcon name="folder" :size="17" class="project-folder" />
                  <button
                    class="project-select"
                    :disabled="store.busy"
                    :draggable="!store.busy"
                    :title="project.path"
                    @click="activateProject(project.id)"
                    @dragstart.stop="startDrag($event, 'project', project.id)"
                    @dragend="endDrag"
                  >
                    <span>{{ project.name }} <small v-if="project.hidden" class="hidden-project-label">{{ t('sidebar.hiddenProject') }}</small> <i v-if="activities.get(project.id) && !(store.active?.id === project.id && activities.get(project.id) === 'unread')" class="activity-dot" :class="activities.get(project.id)" /></span
                    ><small v-if="git.statuses.get(project.id)?.isRepo" :title="git.statuses.get(project.id)?.truncated ? t('sidebar.changesTruncated') : ''">{{ git.statuses.get(project.id)?.branch ?? 'detached' }} · {{ git.statuses.get(project.id)?.files.length }}{{ git.statuses.get(project.id)?.truncated ? '+' : '' }} <template v-if="git.statuses.get(project.id)?.ahead">↑{{ git.statuses.get(project.id)?.ahead }}</template> <template v-if="git.statuses.get(project.id)?.behind">↓{{ git.statuses.get(project.id)?.behind }}</template></small><small>{{ project.path }}</small>
                  </button>
                  <NButton
                    text
                    class="favorite-button" :class="{ 'is-favorite': project.favorite }"
                    :title="project.favorite ? t('sidebar.unfavorite') : t('sidebar.favorite')"
                    :disabled="store.busy"
                    :aria-label="project.favorite ? t('sidebar.unfavorite') : t('sidebar.favorite')"
                    @click="toggleFavorite(project)"
                    ><UiIcon name="star" :size="15" /></NButton
                  >
                </div>
                <p v-if="!visibleProjects(group.id).length" class="empty-group">
                  {{
                    search || favoritesOnly || store.projects.some((p) => (p.groupId ?? '') === group.id)
                      ? t('sidebar.noMatch')
                      : t('sidebar.empty')
                  }}
                </p>
              </div>
            </section>
          </div>
          <p v-if="store.active && git.errors.get(store.active.id)" class="sidebar-tip">{{ git.errors.get(store.active.id) }}</p>
          <details class="sidebar-help"><summary>{{ t('sidebar.help') }}</summary><p class="sidebar-tip">
            {{ t('sidebar.helpBody') }}
          </p></details>
      <ProjectContextMenu trigger="manual" placement="bottom-start" :show="!!contextMenu" :x="contextMenu?.x ?? 0" :y="contextMenu?.y ?? 0" :options="menuOptions" @select="onMenuSelect" @clickoutside="contextMenu = null" />
</template>
