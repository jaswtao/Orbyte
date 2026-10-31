<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NAlert, NButton, NModal } from 'naive-ui'
import ConnectionDialog from './ConnectionDialog.vue'
import { remoteApi, type SshConnection } from '../../services/remoteApi'
import { clearRemoteLink, remoteLinks } from '../../services/remote/status'
import { endpointLabel } from '../../services/remote/workspace'
import { t, type MessageKey } from '../../i18n'

defineProps<{ activeId: string | null }>()
const emit = defineEmits<{ open: [connection: SshConnection]; error: [message: string]; deleted: [id: string] }>()
const connections = ref<SshConnection[]>([])
const dialog = ref(false)
const editing = ref<SshConnection | null>(null)
const deleting = ref<SshConnection | null>(null)
const busy = ref(false)
const error = ref('')
const linkText: Record<string, MessageKey> = {
  idle: 'remote.status.idle',
  connecting: 'remote.status.connecting',
  verifyingHost: 'remote.status.verifyingHost',
  authenticating: 'remote.status.authenticating',
  ready: 'remote.status.ready',
  failed: 'remote.status.failed',
  closed: 'remote.status.closed',
  closing: 'remote.status.closed',
}
function statusLabel(id: string) {
  const state = remoteLinks[id]?.state
  const key = state ? linkText[state] : undefined
  return t(key ?? 'remote.status.idle')
}
async function reload() {
  try { connections.value = await remoteApi.listConnections() }
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
    clearRemoteLink(connection.id)
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
      <button v-for="connection in connections" :key="connection.id" type="button" class="remote-connection" :class="{ active: connection.id === activeId }" role="listitem" @click="emit('open', connection)">
        <span class="remote-connection-name">{{ connection.name }}</span>
        <span class="remote-connection-endpoint">{{ endpointLabel(connection.username, connection.host, connection.port) }}</span>
        <span class="remote-connection-status">{{ statusLabel(connection.id) }}</span>
        <span v-if="remoteLinks[connection.id]?.message" class="remote-connection-message">{{ remoteLinks[connection.id]?.message }}</span>
        <span class="remote-connection-actions">
          <span role="button" tabindex="0" @click.stop="edit(connection)" @keydown.enter.stop="edit(connection)">{{ t('remote.edit') }}</span>
          <span role="button" tabindex="0" @click.stop="deleting = connection" @keydown.enter.stop="deleting = connection">{{ t('remote.delete') }}</span>
        </span>
      </button>
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
