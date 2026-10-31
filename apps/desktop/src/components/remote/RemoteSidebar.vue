<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NButton, NInput, NModal } from 'naive-ui'
import ConnectionManager from './ConnectionManager.vue'
import { useRemote } from '../../stores/remote'
import { remoteApi, type RemoteProject } from '../../services/remoteApi'
import { t } from '../../i18n'
import './remote.css'

const emit = defineEmits<{ error: [message: string] }>()
const remote = useRemote()
const manager = ref(false)
const editing = ref<RemoteProject | null>(null)
const projectDialog = ref(false)
const name = ref('')
const deleting = ref<RemoteProject | null>(null)
const busy = ref(false)
function fail(reason: unknown) { emit('error', String(reason)) }
async function reload() { try { await remote.reload() } catch (reason) { fail(reason) } }
function edit(project?: RemoteProject) {
  editing.value = project ?? null
  name.value = project?.name ?? ''
  projectDialog.value = true
}
async function save() {
  if (busy.value || !name.value.trim()) return
  busy.value = true
  try {
    const id = await remoteApi.saveProject(editing.value?.id ?? null, name.value.trim())
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
onMounted(reload)
</script>
<template>
  <div class="remote-sidebar">
    <div class="sidebar-heading">
      <strong>{{ t('remote.projects') }}</strong>
      <NButton size="small" @click="edit()">{{ t('remote.addProject') }}</NButton>
    </div>
    <p v-if="!remote.projects.length" class="remote-empty">{{ t('remote.noProjects') }}</p>
    <div class="project-list" role="list">
      <div v-for="project in remote.projects" :key="project.id" class="remote-project" :class="{ active: project.id === remote.activeProjectId }" role="listitem">
        <button class="remote-project-select" :aria-pressed="project.id === remote.activeProjectId" @click="remote.selectProject(project.id)">
          <strong>{{ project.name }}</strong>
          <span>{{ t('remote.connectionCount', { count: remote.tabs.filter((tab) => tab.projectId === project.id).length }) }}</span>
        </button>
        <div class="remote-project-actions">
          <NButton size="tiny" @click="edit(project)">{{ t('remote.renameProject') }}</NButton>
          <NButton size="tiny" @click="deleting = project">{{ t('common.delete') }}</NButton>
        </div>
      </div>
    </div>
    <NButton class="remote-manage-button" @click="manager = true">{{ t('remote.manageConnections') }}</NButton>
    <NModal v-model:show="manager" preset="card" :title="t('remote.manageConnections')" class="form-modal">
      <ConnectionManager @changed="reload" @error="emit('error', $event)" />
    </NModal>
    <NModal v-model:show="projectDialog" preset="card" :title="t(editing ? 'remote.renameProject' : 'remote.addProject')" class="form-modal" :mask-closable="!busy">
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
