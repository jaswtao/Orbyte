<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NButton, NInput, NModal } from 'naive-ui'
import ConnectionManager from './ConnectionManager.vue'
import ProjectContextMenu from '../sidebar/ProjectContextMenu.vue'
import { useRemote } from '../../stores/remote'
import { remoteApi, type RemoteProject } from '../../services/remoteApi'
import { copyName, t } from '../../i18n'
import './remote.css'

const emit = defineEmits<{ error: [message: string] }>()
const remote = useRemote()
const manager = ref(false)
const editing = ref<RemoteProject | null>(null)
const duplicating = ref<RemoteProject | null>(null)
const projectDialog = ref(false)
const name = ref('')
const deleting = ref<RemoteProject | null>(null)
const busy = ref(false)
const projectList = ref<HTMLElement>()
const contextMenu = ref<{ id: string; x: number; y: number } | null>(null)
const menuOptions = computed(() => [
  { key: 'rename', label: t('remote.renameProject'), disabled: busy.value },
  { key: 'duplicate', label: t('common.duplicate'), disabled: busy.value },
  { key: 'delete', label: t('remote.deleteProject'), disabled: busy.value },
])
function openMenu(event: MouseEvent, project: RemoteProject) {
  contextMenu.value = busy.value ? null : { id: project.id, x: event.clientX, y: event.clientY }
}
function onMenuSelect(key: string) {
  const project = remote.projects.find((item) => item.id === contextMenu.value?.id)
  contextMenu.value = null
  if (!project || busy.value) return
  if (key === 'rename') edit(project)
  else if (key === 'duplicate') duplicate(project)
  else if (key === 'delete') deleting.value = project
}
function fail(reason: unknown) { emit('error', String(reason)) }
async function reload() { try { await remote.reload() } catch (reason) { fail(reason) } }
function edit(project?: RemoteProject) {
  duplicating.value = null
  editing.value = project ?? null
  name.value = project?.name ?? ''
  projectDialog.value = true
}
function duplicate(project: RemoteProject) {
  editing.value = null
  duplicating.value = project
  name.value = copyName(project.name)
  projectDialog.value = true
}
async function save() {
  if (busy.value || !name.value.trim()) return
  busy.value = true
  try {
    const id = duplicating.value ? await remoteApi.duplicateProject(duplicating.value.id, name.value.trim()) : await remoteApi.saveProject(editing.value?.id ?? null, name.value.trim())
    await remote.reload()
    remote.selectProject(id)
    projectDialog.value = false
  } catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function remove() {
  if (!deleting.value || busy.value) return
  busy.value = true
  try {
    await remoteApi.deleteProject(deleting.value.id)
    await remote.reload()
    deleting.value = null
  } catch (reason) { fail(reason) }
  finally { busy.value = false }
}
function revealProject(id: string) {
  const row = Array.from(projectList.value?.querySelectorAll<HTMLElement>('.remote-project') ?? []).find((element) => element.dataset.projectId === id)
  row?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
}
defineExpose({ revealProject })
onMounted(reload)
</script>
<template>
  <div class="remote-sidebar">
    <div class="sidebar-heading">
      <strong>{{ t('remote.projects') }}</strong>
      <NButton size="small" @click="edit()">{{ t('remote.addProject') }}</NButton>
    </div>
    <p v-if="!remote.projects.length" class="remote-empty">{{ t('remote.noProjects') }}</p>
    <div ref="projectList" class="project-list" role="list">
      <div v-for="project in remote.projects" :key="project.id" :data-project-id="project.id" class="remote-project" :class="{ active: project.id === remote.activeProjectId }" role="listitem" @contextmenu.prevent.stop="openMenu($event, project)">
        <button class="remote-project-select" :aria-pressed="project.id === remote.activeProjectId" @click="remote.selectProject(project.id)">
          <strong>{{ project.name }}</strong>
          <span>{{ t('remote.terminalCount', { count: remote.terminalCounts.get(project.id) ?? project.terminalCount ?? 0 }) }}</span>
        </button>
        <div class="remote-project-actions">
          <NButton size="tiny" @click="edit(project)">{{ t('remote.renameProject') }}</NButton>
          <NButton size="tiny" @click="deleting = project">{{ t('common.delete') }}</NButton>
        </div>
      </div>
    </div>
    <ProjectContextMenu trigger="manual" placement="bottom-start" :show="!!contextMenu" :x="contextMenu?.x ?? 0" :y="contextMenu?.y ?? 0" :options="menuOptions" @select="onMenuSelect" @clickoutside="contextMenu = null" />
    <NButton class="remote-manage-button" @click="manager = true">{{ t('remote.manageConnections') }}</NButton>
    <NModal v-model:show="manager" preset="card" :title="t('remote.manageConnections')" class="form-modal">
      <ConnectionManager @changed="reload" @error="emit('error', $event)" />
    </NModal>
    <NModal v-model:show="projectDialog" preset="card" :title="t(editing ? 'remote.renameProject' : duplicating ? 'common.duplicate' : 'remote.addProject')" class="form-modal" :mask-closable="!busy">
      <form @submit.prevent="save">
        <NInput v-model:value="name" :maxlength="100" :placeholder="t('remote.projectName')" :disabled="busy" />
        <div class="dialog-actions">
          <NButton :disabled="busy" @click="projectDialog = false">{{ t('common.cancel') }}</NButton>
          <NButton attr-type="submit" type="primary" :disabled="!name.trim() || busy" :loading="busy">{{ t('common.save') }}</NButton>
        </div>
      </form>
    </NModal>
    <NModal :show="!!deleting" preset="card" :title="t('remote.deleteProject')" class="form-modal" :mask-closable="!busy" @update:show="(show) => { if (!show && !busy) deleting = null }">
      <p>{{ t('remote.deleteProjectBody', { name: deleting?.name ?? '' }) }}</p>
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="deleting = null">{{ t('common.cancel') }}</NButton>
        <NButton type="error" :loading="busy" @click="remove">{{ t('common.delete') }}</NButton>
      </div>
    </NModal>
  </div>
</template>
