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
const deleting = ref<SshConnection | null>(null)
const busy = ref(false)
const error = ref('')
async function reload() {
  try { connections.value = await remoteApi.listConnections(); emit('changed') }
  catch (reason) { error.value = String(reason); emit('error', error.value) }
}
async function edit(connection?: SshConnection) {
  try {
    if (connection) {
      connections.value = await remoteApi.listConnections()
      const current = connections.value.find((item) => item.id === connection.id)
      if (!current) return
      editing.value = current
    } else editing.value = null
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
      <div v-for="connection in connections" :key="connection.id" class="remote-connection" role="listitem">
        <span class="remote-connection-name">{{ connection.name }}</span>
        <span class="remote-connection-endpoint">{{ endpointLabel(connection.username, connection.host, connection.port) }}</span>
        <span class="remote-connection-actions">
          <NButton size="tiny" @click="edit(connection)">{{ t('remote.edit') }}</NButton>
          <NButton size="tiny" @click="deleting = connection">{{ t('remote.delete') }}</NButton>
        </span>
      </div>
    </div>
    <ConnectionDialog v-model:show="dialog" :connection="editing" @saved="reload" @error="emit('error', $event)" />
    <NModal :show="!!deleting" preset="card" :title="t('remote.delete')" class="form-modal" @update:show="(show) => { if (!show) deleting = null }">
      <p>{{ t('remote.deleteBody', { name: deleting?.name ?? '' }) }}</p>
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="deleting = null">{{ t('common.cancel') }}</NButton>
        <NButton type="error" :loading="busy" @click="remove">{{ t('common.delete') }}</NButton>
      </div>
    </NModal>
  </div>
</template>
