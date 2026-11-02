<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NAlert, NButton, NModal } from 'naive-ui'
import ConnectionDialog from './ConnectionDialog.vue'
import { remoteApi, type SshConnection } from '../../services/remoteApi'
import { endpointLabel } from '../../services/remote/workspace'
import { t } from '../../i18n'

const emit = defineEmits<{ changed: []; error: [message: string]; deleted: [id: string] }>()
const connections = ref<SshConnection[]>([])
const dialog = ref(false)
const editing = ref<SshConnection | null>(null)
const duplicating = ref<SshConnection | null>(null)
const deleting = ref<SshConnection | null>(null)
const busy = ref(false)
const error = ref('')
const dragging = ref<string | null>(null)
const dropTarget = ref<{ id: string; after: boolean } | null>(null)
const sorting = ref(false)
function canDrop(id: string) {
  return !!dragging.value && dragging.value !== id && !sorting.value
}
function allowDrop(event: DragEvent, id: string) {
  if (!canDrop(id)) return
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
}
function startDrag(event: DragEvent, id: string) {
  if (sorting.value) {
    event.preventDefault()
    return
  }
  dragging.value = id
  event.dataTransfer?.setData('text/plain', id)
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}
function dragOver(event: DragEvent, id: string) {
  if (!canDrop(id)) return
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect()
  dropTarget.value = { id, after: event.clientY >= bounds.top + bounds.height / 2 }
}
async function drop(event: DragEvent, id: string) {
  event.preventDefault()
  const source = event.dataTransfer?.getData('text/plain') || dragging.value
  const target = dropTarget.value
  dragging.value = null
  dropTarget.value = null
  if (!source || !target || target.id !== id || source === id || sorting.value) return
  const reordered = [...connections.value]
  const sourceIndex = reordered.findIndex((item) => item.id === source)
  if (sourceIndex < 0) return
  const [item] = reordered.splice(sourceIndex, 1)
  const targetIndex = reordered.findIndex((connection) => connection.id === id)
  if (targetIndex < 0) return
  reordered.splice(targetIndex + (target.after ? 1 : 0), 0, item)
  if (reordered.every((connection, index) => connection.id === connections.value[index].id)) return
  connections.value = reordered
  sorting.value = true
  try {
    await remoteApi.reorderConnections(reordered.map((connection) => connection.id))
    error.value = ''
    emit('changed')
  } catch (reason) {
    await reload()
    error.value = String(reason)
    emit('error', error.value)
  } finally { sorting.value = false }
}
function endDrag() { dragging.value = null; dropTarget.value = null }
async function reload() {
  try { connections.value = await remoteApi.listConnections(); emit('changed') }
  catch (reason) { error.value = String(reason); emit('error', error.value) }
}
async function edit(connection?: SshConnection) {
  try {
    duplicating.value = null
    if (connection) {
      connections.value = await remoteApi.listConnections()
      const current = connections.value.find((item) => item.id === connection.id)
      if (!current) return
      editing.value = current
    } else editing.value = null
    dialog.value = true
  } catch (reason) { error.value = String(reason); emit('error', error.value) }
}
async function duplicate(connection: SshConnection) {
  try {
    connections.value = await remoteApi.listConnections()
    const current = connections.value.find((item) => item.id === connection.id)
    if (!current) return
    editing.value = null
    duplicating.value = current
    dialog.value = true
  } catch (reason) { error.value = String(reason); emit('error', error.value) }
}
async function remove() {
  const connection = deleting.value
  if (!connection || busy.value) return
  busy.value = true
  try {
    await remoteApi.deleteConnection(connection.id)
    deleting.value = null
    emit('deleted', connection.id)
    await reload()
  } catch (reason) {
    error.value = String(reason)
    emit('error', error.value)
  } finally { busy.value = false }
}
onMounted(() => { void reload() })
defineExpose({ reload })
</script>
<template>
  <div class="remote-sidebar">
    <div class="sidebar-heading">
      <strong>{{ t('remote.heading') }}</strong>
      <NButton size="small" @click="edit()">{{ t('remote.add') }}</NButton>
    </div>
    <NAlert v-if="error" type="error">{{ error }}</NAlert>
    <p v-if="!connections.length" class="remote-empty">{{ t('remote.empty') }}</p>
    <div v-else class="project-list" role="list">
      <div v-for="connection in connections" :key="connection.id" class="remote-connection" role="listitem"
        :class="{ dragging: dragging === connection.id, 'drop-before': dropTarget?.id === connection.id && !dropTarget.after, 'drop-after': dropTarget?.id === connection.id && dropTarget.after }"
        @dragenter="allowDrop($event, connection.id)" @dragover="dragOver($event, connection.id)" @drop="drop($event, connection.id)">
        <span class="remote-connection-name remote-connection-sortable" :draggable="!sorting" @dragstart.stop="startDrag($event, connection.id)" @dragend="endDrag">{{ connection.name }}</span>
        <span class="remote-connection-endpoint">{{ endpointLabel(connection.username, connection.host, connection.port) }}</span>
        <span class="remote-connection-actions">
          <NButton size="tiny" @click="edit(connection)">{{ t('remote.edit') }}</NButton>
          <NButton size="tiny" @click="duplicate(connection)">{{ t('common.duplicate') }}</NButton>
          <NButton size="tiny" @click="deleting = connection">{{ t('remote.delete') }}</NButton>
        </span>
      </div>
    </div>
    <ConnectionDialog v-model:show="dialog" :connection="editing" :duplicate-source="duplicating" @saved="reload" @error="emit('error', $event)" />
    <NModal :show="!!deleting" preset="card" :title="t('remote.delete')" class="form-modal" @update:show="(show) => { if (!show) deleting = null }">
      <p>{{ t('remote.deleteBody', { name: deleting?.name ?? '' }) }}</p>
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="deleting = null">{{ t('common.cancel') }}</NButton>
        <NButton type="error" :loading="busy" @click="remove">{{ t('common.delete') }}</NButton>
      </div>
    </NModal>
  </div>
</template>
