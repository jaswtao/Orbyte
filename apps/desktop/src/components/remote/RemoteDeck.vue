<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { NButton, NModal, NSelect } from 'naive-ui'
import RemoteWorkspace from './RemoteWorkspace.vue'
import ConnectionManager from './ConnectionManager.vue'
import { useRemote } from '../../stores/remote'
import type { Settings } from '../../services/projectApi'
import type { AppearanceMode } from '../../services/appearance'
import { endpointLabel } from '../../services/remote/workspace'
import { remoteLinks } from '../../services/remote/status'
import { t } from '../../i18n'

defineProps<{ active: boolean; settings: Settings; appearance: AppearanceMode; shortcutsBlocked: boolean }>()
const emit = defineEmits<{ error: [message: string] }>()
const remote = useRemote()
const choosing = ref(false)
const manager = ref(false)
const chosen = ref<string | null>(null)
const closing = ref<string | null>(null)
const busy = ref(false)
const views = new Map<string, InstanceType<typeof RemoteWorkspace>>()
const retained = computed(() => remote.tabs.filter((tab) => remote.visited.has(tab.id)))
const connectionOptions = computed(() => remote.connections.map((connection) => ({
  value: connection.id, label: `${connection.name} · ${endpointLabel(connection.username, connection.host, connection.port)}`,
})))
function connection(id: string) { return remote.connections.find((item) => item.id === id)! }
function fail(reason: unknown) { emit('error', String(reason)) }
function selectTab(id: string) {
  const same = id === remote.activeTabId
  remote.selectTab(id)
  if (same) void views.get(id)?.connectMissing().catch(fail)
}
function focusTerminal() { if (remote.activeTabId) views.get(remote.activeTabId)?.focusTerminal() }
function renameActiveTab() { return !!remote.activeTabId && (views.get(remote.activeTabId)?.renameActiveTab() ?? false) }
function middleClose(id: string) { if (!busy.value) closing.value = id }
defineExpose({ focusTerminal, renameActiveTab })
async function choose() {
  try { await remote.reload(); chosen.value = null; choosing.value = true }
  catch (reason) { fail(reason) }
}
async function open() {
  if (!chosen.value || busy.value) return
  busy.value = true
  try { await remote.openConnection(chosen.value); choosing.value = false }
  catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function close() {
  if (!closing.value || busy.value) return
  busy.value = true
  try { await remote.closeTab(closing.value); closing.value = null }
  catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function refreshConnections() {
  try { await remote.reload() } catch (reason) { fail(reason) }
}
</script>
<template>
  <div v-show="active" class="remote-deck">
    <Teleport v-if="active" to="#remote-titlebar-tabs">
      <div class="workspace-tabs remote-connection-tabs" role="tablist" :aria-label="t('remote.connectionTabs')">
        <div v-for="tab in remote.projectTabs" :key="tab.id" class="workspace-tab" :class="{ active: tab.id === remote.activeTabId }" @mousedown.middle.prevent @auxclick.middle.prevent.stop="middleClose(tab.id)">
          <button role="tab" :aria-selected="tab.id === remote.activeTabId" :title="endpointLabel(connection(tab.connectionId).username, connection(tab.connectionId).host, connection(tab.connectionId).port)" @click="selectTab(tab.id)"><span class="remote-status-dot" :class="remoteLinks[tab.id]?.state" />{{ connection(tab.connectionId).name }}<span v-if="remote.projectTabs.filter((item) => item.connectionId === tab.connectionId).length > 1"> · {{ remote.projectTabs.filter((item) => item.connectionId === tab.connectionId).findIndex((item) => item.id === tab.id) + 1 }}</span></button>
          <button :aria-label="t('remote.closeConnection')" :disabled="busy" @click="closing = tab.id">×</button>
        </div>
        <button class="workspace-tab-add" :disabled="!remote.activeProjectId" :title="t('remote.openConnection')" :aria-label="t('remote.openConnection')" @click="choose">+</button>
      </div>
    </Teleport>
    <div v-if="!remote.activeTabId || !remote.visited.has(remote.activeTabId)" class="workspace-placeholder">
      <div class="welcome-copy"><h1>{{ t('remote.projects') }}</h1><p>{{ t(remote.activeProjectId ? 'remote.projectPlaceholder' : 'remote.noProjects') }}</p></div>
      <NButton v-if="remote.activeTabId" type="primary" @click="remote.selectTab(remote.activeTabId)">{{ t('remote.reconnect') }}</NButton>
      <NButton v-if="remote.activeProjectId" @click="choose">{{ t('remote.openConnection') }}</NButton>
    </div>
    <RemoteWorkspace v-for="tab in retained" v-show="active && tab.id === remote.activeTabId" :key="tab.id" :ref="(view) => { if (view) views.set(tab.id, view as InstanceType<typeof RemoteWorkspace>); else views.delete(tab.id) }" :workspace-id="tab.id" :connection="connection(tab.connectionId)" :settings="settings" :appearance="appearance" :active="active && tab.id === remote.activeTabId" :shortcuts-blocked="shortcutsBlocked || choosing || manager || !!closing" @error="emit('error', $event)" />
    <NModal v-model:show="choosing" preset="card" :title="t('remote.openConnection')" class="form-modal" :mask-closable="!busy">
      <p>{{ t('remote.chooseConnection') }}</p>
      <NSelect v-model:value="chosen" :options="connectionOptions" filterable :disabled="busy" />
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="manager = true">{{ t('remote.manageConnections') }}</NButton>
        <NButton :disabled="busy" @click="choosing = false">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :loading="busy" :disabled="!chosen || busy" @click="open">{{ t('remote.connect') }}</NButton>
      </div>
    </NModal>
    <NModal v-model:show="manager" preset="card" :title="t('remote.manageConnections')" class="form-modal">
      <ConnectionManager @changed="refreshConnections" @error="emit('error', $event)" />
    </NModal>
    <NModal :show="!!closing" preset="card" :title="t('remote.closeConnection')" class="form-modal" :mask-closable="!busy" @update:show="(show) => { if (!show && !busy) closing = null }">
      <p>{{ t('remote.closeConnectionBody') }}</p>
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="closing = null">{{ t('common.cancel') }}</NButton>
        <NButton type="error" :loading="busy" @click="close">{{ t('common.confirm') }}</NButton>
      </div>
    </NModal>
  </div>
</template>
