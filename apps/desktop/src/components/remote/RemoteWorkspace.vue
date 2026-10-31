<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { NAlert, NButton, NCheckbox, NInput, NModal } from 'naive-ui'
import RemoteLayoutPane from './RemoteLayoutPane.vue'
import { remoteApi, type DirBatch, type DirEntry, type RemoteWorkspace as WorkspaceRecord, type SshConnection, type SshUiEvent, type TransferView } from '../../services/remoteApi'
import { terminalApi, type TerminalSessionInfo } from '../../services/terminalApi'
import { TerminalRegistry } from '../../services/terminal/registry'
import { INITIAL_SIZE } from '../../services/terminal/terminalClient'
import type { AppearanceMode } from '../../services/appearance'
import type { Settings } from '../../services/projectApi'
import { leaves, removeFromLayout, setRatio, splitLeaf, type Layout } from '../../services/workspace/layout'
import { batchCancelId, endpointLabel, failedRoots, formatBytes, joinRemote, parentPath, partitionSessions, remotePaneNeedsCloseConfirm, transferPercent } from '../../services/remote/workspace'
import { isSftpReady, markSftpReady, setRemoteLink } from '../../services/remote/status'
import { t, type MessageKey } from '../../i18n'
import '@xterm/xterm/css/xterm.css'
import '../workspace/workspace.css'
import './remote.css'

const props = defineProps<{ connection: SshConnection; settings: Settings; appearance: AppearanceMode; active: boolean; shortcutsBlocked: boolean }>()
const emit = defineEmits<{ error: [message: string] }>()
const record = ref<WorkspaceRecord | null>(null)
const focused = ref<string | null>(null)
const entries = ref<DirEntry[]>([])
const pathInput = ref('.')
const browsing = ref(false)
const sftpUp = ref(false)
const selected = ref<string[]>([])
const transfers = ref<TransferView[]>([])
const sftpError = ref('')
const credential = ref<{ batchId: string; authType: string } | null>(null)
const secret = ref('')
const challenge = ref<SshUiEvent | null>(null)
const closing = ref<{ title: string; ids: string[]; tabId?: string } | null>(null)
const folderOpen = ref(false)
const folderName = ref('')
const applyAll = ref(false)
const busy = ref(false)
const launching = ref(false)
let heldCredential: { batchId: string; authType: string } | null = null
let sftpAttemptId = ''
let browseId = ''
let browseGeneration = 0
let reloadToken = 0
let ratioTimer = 0
const dialogOpen = computed(() => !!credential.value || !!challenge.value || !!closing.value || folderOpen.value || !!conflictTransfer.value)
const registry = new TerminalRegistry(() => props.appearance, () => props.settings, () => props.shortcutsBlocked || dialogOpen.value)
const endpoint = computed(() => endpointLabel(props.connection.username, props.connection.host, props.connection.port))
const layout = computed(() => record.value ? asLayout(record.value.layout) : null)
const activeTab = computed(() => layout.value?.tabs.find((tab) => tab.id === layout.value?.activeTabId) ?? null)
const names = computed(() => new Map((record.value?.terminals ?? []).map((terminal) => [terminal.id, terminal.name])))
const parent = computed(() => parentPath(pathInput.value))
const workspaceTransfers = computed(() => transfers.value.filter((transfer) => transfer.workspaceId === record.value?.id))
const conflictTransfer = computed(() => workspaceTransfers.value.find((transfer) => transfer.state === 'awaitingConflict') ?? null)
const conflictName = computed(() => conflictTransfer.value?.files.find((file) => file.status === 'awaitingConflict')?.relative || conflictTransfer.value?.message || '')
const stateText: Record<string, MessageKey> = {
  queued: 'remote.state.queued', scanning: 'remote.state.scanning', running: 'remote.state.running', completed: 'remote.state.completed',
  awaitingConflict: 'remote.state.awaitingConflict', cancelling: 'remote.state.cancelling', cancelled: 'remote.state.cancelled', failed: 'remote.state.failed',
}
const kindText: Record<string, MessageKey> = {
  dir: 'remote.kind.dir', file: 'remote.kind.file', symlink: 'remote.kind.symlink', other: 'remote.kind.other', unsupported: 'remote.kind.unsupported',
}
const directionText: Record<string, MessageKey> = { upload: 'remote.direction.upload', download: 'remote.direction.download' }

function asLayout(value: WorkspaceRecord['layout']): Layout {
  return value as unknown as Layout
}
function terminalIds(value: Layout): string[] {
  return value.tabs.flatMap((tab) => leaves(tab.root).flatMap((leaf) => leaf.type === 'terminal' ? [leaf.terminalConfigId] : []))
}
function fail(reason: unknown) {
  const message = String(reason)
  emit('error', message)
  return message
}
function disposeEntries() {
  for (const id of [...registry.byConfig.keys()]) registry.remove(id)
}
function ensureEntry(configId: string) {
  const existing = registry.byConfig.get(configId)
  if (existing) return existing
  return registry.create(
    { fontFamily: props.settings.font_family, fontSize: props.settings.font_size, scrollback: props.settings.scrollback },
    (message) => emit('error', message),
    undefined,
    undefined,
    (message) => emit('error', message),
    true,
  )
}
async function ensureTerminal(loaded: WorkspaceRecord): Promise<WorkspaceRecord> {
  if (loaded.layout.tabs.length) return loaded
  const name = t('remote.terminal')
  const configId = await remoteApi.createTerminal(loaded.id, name)
  const tabId = crypto.randomUUID()
  const next = {
    schemaVersion: 1,
    activeTabId: tabId,
    tabs: [{ id: tabId, title: name, root: { type: 'terminal' as const, terminalConfigId: configId } }],
  }
  await remoteApi.saveWorkspace(loaded.id, next)
  return { ...loaded, layout: next, terminals: [...loaded.terminals, { id: configId, remoteWorkspaceId: loaded.id, name, sortOrder: loaded.terminals.length }] }
}
async function persist(next: Layout) {
  if (!record.value) return
  const layoutValue = next as unknown as WorkspaceRecord['layout']
  record.value = { ...record.value, layout: layoutValue }
  await remoteApi.saveWorkspace(record.value.id, layoutValue)
}
function onUi(event: SshUiEvent) {
  if (event.type === 'challenge' && event.challengeId) {
    challenge.value = event
    return
  }
  if (event.type === 'credentialRequired' && event.batchId) {
    const pending = { batchId: event.batchId, authType: event.authType || props.connection.authType }
    if (launching.value) heldCredential = pending
    else credential.value = pending
    return
  }
  if (event.type !== 'attempt') return
  setRemoteLink(props.connection.id, event.state || 'idle', event.message || '')
    if (event.attemptId && event.attemptId === sftpAttemptId) {
    if (event.state === 'ready' && record.value) {
      markSftpReady(record.value.id, true)
      sftpUp.value = true
      sftpError.value = ''
      void browse(record.value.lastDirectory || pathInput.value || '.')
    } else if (event.state === 'failed') {
      if (record.value) markSftpReady(record.value.id, false)
      sftpUp.value = false
      sftpError.value = event.message || t('remote.sftpFailed')
    }
  }
}
async function launch(configIds: string[], includeSftp: boolean) {
  if (launching.value || !record.value || (!configIds.length && !includeSftp)) return
  launching.value = true
  heldCredential = null
  const events = new Channel<SshUiEvent>()
  events.onmessage = (event) => onUi(event)
  let batchId = ''
  try {
    const batch = await remoteApi.beginConnect(props.connection.authType, events)
    batchId = batch.batchId
    for (const configId of configIds) {
      const entry = ensureEntry(configId)
      const previous = entry.client.id
      if (previous) {
        entry.intentionalStop = true
        try { await terminalApi.stopTerminal(previous) } catch { /* The old session may already be gone. */ }
      }
      const channel = entry.client.createChannel()
      const created = await remoteApi.connectTerminal(batchId, props.connection.id, configId, entry.client.measure() ?? INITIAL_SIZE, channel)
      registry.bind(configId, entry, created.terminalId)
    }
    if (includeSftp && record.value) {
      const connected = await remoteApi.connectSftp(batchId, props.connection.id, record.value.id)
      sftpAttemptId = connected.attemptId
    }
  } catch (reason) {
    if (batchId) await remoteApi.cancelAttempt(batchCancelId(batchId)).catch(() => {})
    fail(reason)
  } finally {
    launching.value = false
    if (heldCredential) {
      credential.value = heldCredential
      heldCredential = null
    }
    syncVisibility()
  }
}
async function connectMissing() {
  if (!record.value || launching.value) return
  const current = asLayout(record.value.layout)
  const sessions = await remoteApi.listSessions()
  const { attach, connect } = partitionSessions(terminalIds(current), sessions)
  for (const item of attach) {
    const entry = ensureEntry(item.configId)
    if (entry.client.id === item.terminalId) continue
    const channel = entry.client.createChannel()
    await terminalApi.subscribeTerminal(item.terminalId, channel)
    registry.bind(item.configId, entry, item.terminalId)
    const session = sessions.find((candidate) => candidate.terminalId === item.terminalId)
    if (session) entry.client.updateStatus(session.status)
  }
  const needSftp = !isSftpReady(record.value.id)
  sftpUp.value = !needSftp
  if (!connect.length && !needSftp) {
    await browse(record.value.lastDirectory || '.')
    return
  }
  await launch(connect, needSftp)
}
async function reload() {
  reloadToken += 1
  const token = reloadToken
  disposeEntries()
  entries.value = []
  selected.value = []
  sftpError.value = ''
  try {
    const loaded = await ensureTerminal(await remoteApi.loadWorkspace(props.connection.id))
    if (token !== reloadToken) return
    record.value = loaded
    pathInput.value = loaded.lastDirectory || '.'
    focused.value = terminalIds(asLayout(loaded.layout))[0] ?? null
    await connectMissing()
  } catch (reason) { if (token === reloadToken) fail(reason) }
}
async function browse(path: string) {
  if (!record.value || !isSftpReady(record.value.id)) return
  const generation = ++browseGeneration
  if (browseId) void remoteApi.cancelBrowse(browseId)
  browsing.value = true
  selected.value = []
  entries.value = []
  const workspaceId = record.value.id
  const channel = new Channel<DirBatch>()
  channel.onmessage = (batch) => {
    if (generation !== browseGeneration) return
    if (batch.entries.length) entries.value = [...entries.value, ...batch.entries]
    if (batch.path) pathInput.value = batch.path
    if (!batch.done) return
    browsing.value = false
    sftpError.value = batch.message || ''
    if (!batch.message && batch.path) void remoteApi.setDirectory(workspaceId, batch.path).catch((reason) => fail(reason))
  }
  try {
    const requestId = await remoteApi.listDirectory(workspaceId, path, channel)
    if (generation !== browseGeneration) {
      void remoteApi.cancelBrowse(requestId)
      return
    }
    browseId = requestId
  } catch (reason) {
    if (generation === browseGeneration) {
      browsing.value = false
      sftpError.value = fail(reason)
    }
  }
}
function openEntry(entry: DirEntry) {
  if (entry.kind !== 'dir') return
  void browse(joinRemote(pathInput.value, entry.name))
}
function toggleSelect(name: string) {
  selected.value = selected.value.includes(name) ? selected.value.filter((item) => item !== name) : [...selected.value, name]
}
async function submitPath() {
  if (!record.value) return
  try {
    const canonical = await remoteApi.canonicalize(record.value.id, pathInput.value)
    await browse(canonical)
  } catch (reason) { sftpError.value = fail(reason) }
}
async function makeDirectory() {
  if (!record.value || !folderName.value.trim()) return
  busy.value = true
  try {
    await remoteApi.createDirectory(record.value.id, joinRemote(pathInput.value, folderName.value.trim()))
    folderOpen.value = false
    folderName.value = ''
    await browse(pathInput.value)
  } catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function transfer(kind: 'upload-files' | 'upload-folders' | 'download', names: string[] = []) {
  if (!record.value) return
  if (kind === 'download' && !names.length) {
    emit('error', t('remote.selectDownload'))
    return
  }
  const downloadNames = names.filter((name) => entries.value.find((entry) => entry.name === name)?.kind !== 'unsupported')
  if (kind === 'download' && !downloadNames.length) {
    emit('error', t('remote.kind.unsupported'))
    return
  }
  try {
    const grant = await remoteApi.selectPaths(record.value.id, kind)
    if (!grant) return
    await remoteApi.enqueueTransfer(record.value.id, grant.token, crypto.randomUUID(), pathInput.value, kind === 'download' ? downloadNames : [])
  } catch (reason) { fail(reason) }
}
async function retry(transferItem: TransferView) {
  if (transferItem.direction === 'download') await transfer('download', failedRoots(transferItem.files))
  else await transfer('upload-files')
}
async function resolveConflict(choice: 'overwrite' | 'skip' | 'cancel') {
  const transferItem = conflictTransfer.value
  if (!transferItem) return
  try {
    await remoteApi.resolveConflict(transferItem.id, choice, applyAll.value)
    applyAll.value = false
  } catch (reason) { fail(reason) }
}
async function submitSecret() {
  const pending = credential.value
  if (!pending) return
  const value = secret.value
  secret.value = ''
  credential.value = null
  try { await remoteApi.submitCredential(pending.batchId, value) }
  catch (reason) { fail(reason) }
}
async function cancelSecret() {
  const pending = credential.value
  secret.value = ''
  credential.value = null
  if (pending) await remoteApi.cancelAttempt(batchCancelId(pending.batchId)).catch((reason) => fail(reason))
}
async function answer(decision: 'once' | 'save' | 'cancel') {
  const current = challenge.value
  challenge.value = null
  if (!current?.challengeId) return
  try { await remoteApi.answerChallenge(current.challengeId, decision) }
  catch (reason) { fail(reason) }
}
function selectTab(id: string) {
  if (!record.value || record.value.layout.activeTabId === id) return
  const next = { ...asLayout(record.value.layout), activeTabId: id }
  void persist(next).catch(fail)
}
function askClose(ids: string[], title: string, tabId?: string) {
  if (!ids.length) return
  closing.value = { ids, title, tabId }
  // The local "confirm before closing panes" setting does not skip this prompt.
  if (!remotePaneNeedsCloseConfirm()) void confirmClose()
}
async function confirmClose() {
  const request = closing.value
  if (!request || !record.value || busy.value) return
  busy.value = true
  try {
    let next = asLayout(record.value.layout)
    for (const id of request.ids) {
      const entry = registry.byConfig.get(id)
      if (entry?.client.id) {
        entry.intentionalStop = true
        try { await terminalApi.stopTerminal(entry.client.id) } catch { /* Closing still removes the saved pane. */ }
      }
      registry.remove(id)
      await remoteApi.removeTerminal(record.value.id, id)
      next = removeFromLayout(next, id)
    }
    if (request.tabId) {
      const tabs = next.tabs.filter((tab) => tab.id !== request.tabId)
      next = { ...next, tabs, activeTabId: tabs.some((tab) => tab.id === next.activeTabId) ? next.activeTabId : tabs[0]?.id ?? null }
    }
    await persist(next)
    focused.value = activeTab.value ? leaves(activeTab.value.root).flatMap((leaf) => leaf.type === 'terminal' ? [leaf.terminalConfigId] : [])[0] ?? null : null
    closing.value = null
  } catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function addTerminal(direction?: 'horizontal' | 'vertical') {
  if (!record.value || !layout.value) return
  const name = t('remote.terminal')
  const configId = await remoteApi.createTerminal(record.value.id, name)
  const current = asLayout(record.value.layout)
  const tab = current.tabs.find((item) => item.id === current.activeTabId)
  if (direction && tab && focused.value) {
    tab.root = splitLeaf(tab.root, focused.value, { type: 'terminal', terminalConfigId: configId }, direction)
  } else {
    const tabId = crypto.randomUUID()
    current.tabs.push({ id: tabId, title: name, root: { type: 'terminal', terminalConfigId: configId } })
    current.activeTabId = tabId
  }
  record.value = { ...record.value, terminals: [...record.value.terminals, { id: configId, remoteWorkspaceId: record.value.id, name, sortOrder: record.value.terminals.length }] }
  await persist(current)
  focused.value = configId
  await launch([configId], !isSftpReady(record.value.id))
}
function onRatio(path: number[], value: number) {
  if (!record.value) return
  const current = asLayout(record.value.layout)
  const tab = current.tabs.find((item) => item.id === current.activeTabId)
  if (!tab) return
  tab.root = setRatio(tab.root, path, value)
  record.value = { ...record.value, layout: current as unknown as WorkspaceRecord['layout'] }
  window.clearTimeout(ratioTimer)
  ratioTimer = window.setTimeout(() => { if (record.value) void persist(asLayout(record.value.layout)).catch(fail) }, 300)
}
function syncVisibility() {
  const ids = new Set(activeTab.value ? leaves(activeTab.value.root).flatMap((leaf) => leaf.type === 'terminal' ? [leaf.terminalConfigId] : []) : [])
  for (const [id, entry] of registry.byConfig) {
    const visible = props.active && ids.has(id)
    if (entry.visible && !visible) entry.host.dispatchEvent(new Event('terminal-session-hidden', { bubbles: true }))
    entry.visible = visible
    entry.client.setVisible(visible)
    if (visible) {
      entry.client.markContentRead()
      entry.unread = false
      entry.client.refreshSize()
    }
  }
}
function progress(item: TransferView) {
  const percent = transferPercent(item.state, item.transferred, item.total)
  const moved = formatBytes(item.transferred)
  const state = t(stateText[item.state] ?? 'remote.state.failed')
  if (percent == null) return `${moved} · ${state}`
  return `${moved}${item.total == null ? '' : ` / ${formatBytes(item.total)}`} · ${percent}% · ${formatBytes(item.bytesPerSecond)}/s`
}
function kindLabel(kind: string) {
  return t(kindText[kind] ?? 'remote.kind.other')
}
async function syncSessions() {
  let sessions: TerminalSessionInfo[]
  try { sessions = await remoteApi.listSessions() }
  catch { return }
  for (const entry of registry.byConfig.values()) {
    const id = entry.client.id
    if (!entry.remote || !id) continue
    const session = sessions.find((item) => item.terminalId === id)
    if (session) entry.client.observeSession(session)
  }
}
async function listenTransfers() {
  const channel = new Channel<{ transfers: TransferView[] }>()
  let live = false
  channel.onmessage = (event) => { live = true; transfers.value = event.transfers }
  try {
    await remoteApi.subscribeTransfers(channel)
    const snapshot = await remoteApi.listTransfers()
    if (!live) transfers.value = snapshot.transfers
  } catch (reason) { fail(reason) }
}
const sessionTimer = window.setInterval(() => { void syncSessions() }, 2000)
watch(() => props.connection.id, () => { void reload() }, { immediate: true })
watch(() => [props.active, record.value?.layout.activeTabId, record.value?.layout.tabs.length], syncVisibility)
watch(() => props.settings, (settings) => registry.updateSettings(settings), { deep: true })
watch(() => props.appearance, () => registry.updateSettings(props.settings))
onMounted(() => { void listenTransfers() })
onBeforeUnmount(() => {
  window.clearInterval(sessionTimer)
  window.clearTimeout(ratioTimer)
  reloadToken += 1
  if (browseId) void remoteApi.cancelBrowse(browseId)
  disposeEntries()
})
defineExpose({ connectMissing })
</script>
<template>
  <section v-if="record && layout" class="remote-workspace">
    <div class="remote-toolbar">
      <span class="remote-title">{{ connection.name }}</span>
      <span class="remote-connection-endpoint">{{ endpoint }}</span>
      <NButton size="small" @click="addTerminal()">{{ t('remote.newTab') }}</NButton>
      <NButton size="small" :disabled="!focused" @click="addTerminal('horizontal')">{{ t('remote.splitHorizontal') }}</NButton>
      <NButton size="small" :disabled="!focused" @click="addTerminal('vertical')">{{ t('remote.splitVertical') }}</NButton>
    </div>
    <div class="remote-tabs">
      <button v-for="tab in layout.tabs" :key="tab.id" type="button" class="workspace-tab" :class="{ active: tab.id === layout.activeTabId }" @click="selectTab(tab.id)">
        {{ tab.title }}
        <span @click.stop="askClose(leaves(tab.root).flatMap((leaf) => leaf.type === 'terminal' ? [leaf.terminalConfigId] : []), t('remote.closeTab', { title: tab.title }), tab.id)">×</span>
      </button>
    </div>
    <div class="remote-body">
      <div class="remote-terminals">
        <RemoteLayoutPane v-if="activeTab" :node="activeTab.root" :path="[]" :registry="registry" :names="names" :focused="focused" :visible="active" :endpoint="endpoint" @focus="focused = $event" @close="askClose([$event], t('remote.closePane'))" @reconnect="(id) => launch([id], false)" @ratio="onRatio" />
      </div>
      <aside class="remote-files">
        <div class="remote-file-toolbar">
          <strong>{{ t('remote.files') }}</strong>
          <NButton size="tiny" :disabled="browsing || !sftpUp" @click="browse(pathInput)">{{ t('remote.refresh') }}</NButton>
          <NButton size="tiny" :disabled="!browsing" @click="browseId && remoteApi.cancelBrowse(browseId)">{{ t('remote.cancelBrowse') }}</NButton>
        </div>
        <div class="remote-path">
          <NButton size="tiny" :disabled="!parent" @click="parent && browse(parent)">{{ t('remote.parent') }}</NButton>
          <NInput v-model:value="pathInput" :placeholder="t('remote.path')" @keydown.enter="submitPath" />
        </div>
        <div class="remote-file-toolbar">
          <NButton size="tiny" @click="transfer('upload-files')">{{ t('remote.uploadFiles') }}</NButton>
          <NButton size="tiny" @click="transfer('upload-folders')">{{ t('remote.uploadFolders') }}</NButton>
          <NButton size="tiny" :disabled="!selected.length" @click="transfer('download', selected)">{{ t('remote.download') }}</NButton>
          <NButton size="tiny" @click="folderOpen = true">{{ t('remote.mkdir') }}</NButton>
          <NButton v-if="!sftpUp" size="tiny" @click="launch([], true)">{{ t('remote.connectSftp') }}</NButton>
        </div>
        <NAlert v-if="sftpError" type="error">{{ sftpError }}</NAlert>
        <div class="remote-listing">
          <div class="remote-head"><span>{{ t('remote.col.name') }}</span><span>{{ t('remote.col.kind') }}</span><span>{{ t('remote.col.size') }}</span><span>{{ t('remote.col.mtime') }}</span></div>
          <button v-for="entry in entries" :key="entry.name" type="button" class="remote-row" :class="{ selected: selected.includes(entry.name) }" @click="toggleSelect(entry.name)" @dblclick="openEntry(entry)">
            <span>{{ entry.name }}</span>
            <span>{{ kindLabel(entry.kind) }}</span>
            <span>{{ entry.kind === 'dir' ? '' : formatBytes(entry.size) }}</span>
            <span>{{ entry.mtime == null ? '—' : new Date(entry.mtime * 1000).toLocaleString() }}</span>
          </button>
        </div>
        <p class="remote-note">{{ t('remote.transferNote') }}</p>
        <div class="remote-transfer">
          <strong>{{ t('remote.transfers') }}</strong>
          <div v-for="item in workspaceTransfers" :key="item.id">
            <div>{{ t(directionText[item.direction] ?? 'remote.direction.upload') }} · {{ progress(item) }}</div>
            <div v-if="item.message">{{ item.message }}</div>
            <NButton size="tiny" @click="remoteApi.cancelTransfer(item.id).catch(fail)">{{ t('remote.cancelTransfer') }}</NButton>
            <NButton v-if="item.state === 'failed' || item.state === 'cancelled' || item.files.some((file) => file.status === 'failed')" size="tiny" @click="retry(item)">{{ t('remote.retryTransfer') }}</NButton>
          </div>
        </div>
      </aside>
    </div>
    <NModal :show="!!credential" preset="card" :title="t('remote.credentialTitle')" class="form-modal" :mask-closable="false" @update:show="(show) => { if (!show) void cancelSecret() }">
      <p>{{ t(credential?.authType === 'privateKey' ? 'remote.passphrasePrompt' : 'remote.passwordPrompt', { endpoint }) }}</p>
      <NInput v-model:value="secret" type="password" show-password-on="click" :placeholder="t('remote.secret')" @keydown.enter="submitSecret" />
      <div class="dialog-actions">
        <NButton @click="cancelSecret">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :disabled="!secret" @click="submitSecret">{{ t('common.confirm') }}</NButton>
      </div>
    </NModal>
    <NModal :show="!!challenge" preset="card" :title="t('remote.challengeTitle')" class="form-modal" :mask-closable="false" @update:show="(show) => { if (!show) void answer('cancel') }">
      <p>{{ t('remote.challengeBody', { host: challenge?.host ?? '', port: challenge?.port ?? '' }) }}</p>
      <p>{{ t('remote.algorithm') }}: {{ challenge?.algorithm }}</p>
      <p>{{ t('remote.fingerprint') }}: {{ challenge?.fingerprint }}</p>
      <div class="dialog-actions">
        <NButton @click="answer('cancel')">{{ t('common.cancel') }}</NButton>
        <NButton @click="answer('once')">{{ t('remote.trustOnce') }}</NButton>
        <NButton type="primary" @click="answer('save')">{{ t('remote.trustSave') }}</NButton>
      </div>
    </NModal>
    <NModal :show="!!closing" preset="card" :title="t('workspace.confirmClose')" class="form-modal" :mask-closable="false" @update:show="(show) => { if (!show && !busy) closing = null }">
      <p>{{ closing?.title }}</p>
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="closing = null">{{ t('common.cancel') }}</NButton>
        <NButton type="error" :loading="busy" @click="confirmClose">{{ t('common.confirm') }}</NButton>
      </div>
    </NModal>
    <NModal v-model:show="folderOpen" preset="card" :title="t('remote.mkdir')" class="form-modal">
      <NInput v-model:value="folderName" :placeholder="t('remote.folderName')" @keydown.enter="makeDirectory" />
      <div class="dialog-actions">
        <NButton @click="folderOpen = false">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :loading="busy" :disabled="!folderName.trim()" @click="makeDirectory">{{ t('common.confirm') }}</NButton>
      </div>
    </NModal>
    <NModal :show="!!conflictTransfer" preset="card" :title="t('remote.conflictTitle')" class="form-modal" :mask-closable="false">
      <p>{{ t('remote.conflictBody', { name: conflictName }) }}</p>
      <NCheckbox v-model:checked="applyAll">{{ t('remote.applyAll') }}</NCheckbox>
      <div class="dialog-actions">
        <NButton @click="resolveConflict('cancel')">{{ t('common.cancel') }}</NButton>
        <NButton @click="resolveConflict('skip')">{{ t('remote.skip') }}</NButton>
        <NButton type="primary" @click="resolveConflict('overwrite')">{{ t('remote.overwrite') }}</NButton>
      </div>
    </NModal>
  </section>
</template>
