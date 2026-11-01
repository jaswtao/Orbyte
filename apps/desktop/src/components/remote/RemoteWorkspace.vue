<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { NAlert, NButton, NCheckbox, NInput, NModal, NSelect } from 'naive-ui'
import RemoteLayoutPane from './RemoteLayoutPane.vue'
import ConnectionManager from './ConnectionManager.vue'
import { remoteApi, type DirBatch, type DirEntry, type RemoteProjectWorkspace as WorkspaceRecord, type RemoteContext, type ProjectTerminal, type SshUiEvent, type TransferView } from '../../services/remoteApi'
import { terminalApi, type TerminalSessionInfo } from '../../services/terminalApi'
import { TerminalRegistry } from '../../services/terminal/registry'
import { INITIAL_SIZE } from '../../services/terminal/terminalClient'
import type { AppearanceMode } from '../../services/appearance'
import type { Settings } from '../../services/projectApi'
import { leaves, setRatio, splitLeaf, type Layout } from '../../services/workspace/layout'
import { matchShortcut, shortcutHint } from '../../services/workspace/shortcuts'
import { batchCancelId, endpointLabel, failedRoots, formatBytes, joinRemote, parentPath, partitionSessions, remoteCdCommand, remotePaneNeedsCloseConfirm, terminalPaneNames, transferPercent } from '../../services/remote/workspace'
import { isSftpReady, markSftpReady } from '../../services/remote/status'
import { t, type MessageKey } from '../../i18n'
import { useRemote } from '../../stores/remote'
import '@xterm/xterm/css/xterm.css'
import '../workspace/workspace.css'
import './remote.css'

const props = defineProps<{ workspaceId: string; settings: Settings; appearance: AppearanceMode; active: boolean; shortcutsBlocked: boolean }>()
const emit = defineEmits<{ error: [message: string] }>()
const record = ref<WorkspaceRecord | null>(null)
const focused = ref<string | null>(null)
const entries = ref<DirEntry[]>([])
const pathInput = ref('.')
const directory = ref('.')
const browsing = ref(false)
const sftpUp = ref(false)
const selected = ref<string[]>([])
const remote = useRemote()
const sftpError = ref('')
const credential = ref<{ batchId: string; authType: string; connectionId: string } | null>(null)
const credentialQueue: NonNullable<typeof credential.value>[] = []
const challengeQueue: SshUiEvent[] = []
const secret = ref('')
const savePassword = ref(false)
const submittingSecret = ref(false)
const challenge = ref<SshUiEvent | null>(null)
const closing = ref<{ title: string; ids: string[]; tabId?: string } | null>(null)
const rename = ref<{ id: string; title: string } | null>(null)
const renameInput = ref<{ focus: () => void; select: () => void } | null>(null)
const savingRename = ref(false)
const folderOpen = ref(false)
const folderName = ref('')
const applyAll = ref(false)
const busy = ref(false)
const launching = ref(false)
const sftpAttempts = new Map<string, string>()
const choosing = ref(false)
const manager = ref(false)
const chosen = ref<string | null>(null)
const creating = ref(false)
const initializing = ref(false)
const refreshing = ref(false)
const refreshPending = ref(false)
let creationTarget: { direction?: 'horizontal' | 'vertical'; tabId: string | null; focused: string | null } | null = null
const connectionOptions = computed(() => remote.connections.map((connection) => ({ value: connection.id, label: `${connection.name} · ${endpointLabel(connection.username, connection.host, connection.port)}` })))
let browseId = ''
let browseGeneration = 0
let reloadToken = 0
let ratioTimer = 0
const dialogOpen = computed(() => choosing.value || manager.value || !!credential.value || !!challenge.value || !!closing.value || !!rename.value || folderOpen.value || !!conflictTransfer.value)
const registry = new TerminalRegistry(() => props.appearance, () => props.settings, () => props.shortcutsBlocked || dialogOpen.value)
const terminalDirectory = computed(() => focused.value ? registry.byConfig.get(focused.value)?.shell.cwd ?? null : null)
const focusedConfig = computed(() => record.value?.terminals.find((terminal) => terminal.id === focused.value))
const connection = computed(() => remote.connections.find((item) => item.id === focusedConfig.value?.connectionId))
const fileContext = computed(() => record.value?.contexts.find((context) => context.connectionId === connection.value?.id) ?? null)
const endpoint = computed(() => connection.value ? endpointLabel(connection.value.username, connection.value.host, connection.value.port) : '')
const endpoints = computed(() => new Map((record.value?.terminals ?? []).map((terminal) => {
  const item = remote.connections.find((connection) => connection.id === terminal.connectionId)
  return [terminal.id, item ? `${item.name} · ${endpointLabel(item.username, item.host, item.port)}` : '']
})))
const credentialEndpoint = computed(() => {
  const item = remote.connections.find((connection) => connection.id === credential.value?.connectionId)
  return item ? endpointLabel(item.username, item.host, item.port) : ''
})
const layout = computed(() => record.value ? asLayout(record.value.layout) : null)
const activeTab = computed(() => layout.value?.tabs.find((tab) => tab.id === layout.value?.activeTabId) ?? null)
const names = computed(() => terminalPaneNames(record.value?.terminals ?? [], layout.value?.tabs ?? []))
const hint = (id: string) => shortcutHint(id, props.settings.shortcuts)
const parent = computed(() => parentPath(directory.value))
const workspaceTransfers = computed(() => remote.transfers.filter((transfer) => record.value?.contexts.some((context) => context.id === transfer.workspaceId)))
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
let saveChain: Promise<unknown> = Promise.resolve()
function enqueueLayoutSave(workspaceId: string, layoutValue: WorkspaceRecord['layout']) {
  // A tab switch already in flight must not land after a rename and put the old title back.
  const operation = saveChain.catch(() => {}).then(() => remoteApi.saveProjectLayout(workspaceId, layoutValue))
  saveChain = operation
  return operation
}
async function persist(next: Layout) {
  if (!record.value) return
  const layoutValue = next as unknown as WorkspaceRecord['layout']
  record.value = { ...record.value, layout: layoutValue }
  await enqueueLayoutSave(record.value.id, layoutValue)
}
function nextPrompts() {
  if (credential.value || challenge.value) return
  challenge.value = challengeQueue.shift() ?? null
  if (!challenge.value && !launching.value) credential.value = credentialQueue.shift() ?? null
}
function onUi(event: SshUiEvent, context: RemoteContext) {
  if (!record.value?.contexts.some((item) => item.id === context.id)) return
  if (event.type === 'challenge' && event.challengeId) {
    challengeQueue.push(event)
    nextPrompts()
    return
  }
  if (event.type === 'credentialRequired' && event.batchId) {
    const item = remote.connections.find((connection) => connection.id === context.connectionId)
    credentialQueue.push({ batchId: event.batchId, authType: event.authType || item?.authType || 'password', connectionId: context.connectionId })
    nextPrompts()
    return
  }
  if (event.type !== 'attempt') return
  if (event.attemptId && event.attemptId === sftpAttempts.get(context.id)) {
    markSftpReady(context.id, event.state === 'ready')
    if (fileContext.value?.id === context.id) {
      sftpUp.value = event.state === 'ready'
      sftpError.value = event.state === 'failed' ? event.message || t('remote.sftpFailed') : ''
      if (sftpUp.value) void browse(terminalDirectory.value || context.lastDirectory || '.')
    }
  }
}
async function launchContext(configIds: string[], context: RemoteContext, includeSftp: boolean) {
  let pendingSftpEvents: SshUiEvent[] | null = includeSftp ? [] : null
  const events = new Channel<SshUiEvent>()
  events.onmessage = (event) => {
    if (pendingSftpEvents && event.type === 'attempt' && !event.terminalId) pendingSftpEvents.push(event)
    else onUi(event, context)
  }
  let batchId = ''
  try {
    const batch = await remoteApi.beginConnect(context.connectionId, events)
    batchId = batch.batchId
    for (const configId of configIds) {
      const entry = ensureEntry(configId)
      const previous = entry.client.id
      if (previous) {
        entry.intentionalStop = true
        try { await terminalApi.stopTerminal(previous) } catch { /* The old session may already be gone. */ }
      }
      const channel = entry.client.createChannel()
      const created = await remoteApi.connectTerminal(batchId, context.connectionId, configId, entry.client.measure() ?? INITIAL_SIZE, channel)
      registry.bind(configId, entry, created.terminalId)
    }
    if (includeSftp) {
      const connected = await remoteApi.connectSftp(batchId, context.connectionId, context.id)
      sftpAttempts.set(context.id, connected.attemptId)
      const pending = pendingSftpEvents
      pendingSftpEvents = null
      for (const event of pending ?? []) onUi(event, context)
    }
    await remoteApi.sealConnect(batchId)
  } catch (reason) {
    if (batchId) await remoteApi.cancelAttempt(batchCancelId(batchId)).catch(() => {})
    fail(reason)
  } finally { pendingSftpEvents = null }
}
async function launch(configIds: string[], includeSftp: boolean) {
  if (launching.value || !record.value) return
  const context = fileContext.value
  if (!configIds.length && (!includeSftp || !context)) return
  launching.value = true
  try {
    const groups = new Map<string, string[]>()
    for (const id of configIds) {
      const config = record.value.terminals.find((terminal) => terminal.id === id)
      if (config) groups.set(config.connectionId, [...(groups.get(config.connectionId) ?? []), id])
    }
    if (includeSftp && context && !groups.has(context.connectionId)) groups.set(context.connectionId, [])
    for (const [connectionId, ids] of groups) {
      const target = record.value.contexts.find((item) => item.connectionId === connectionId)
      if (target) await launchContext(ids, target, includeSftp && connectionId === context?.connectionId)
    }
  } finally {
    launching.value = false
    nextPrompts()
    syncVisibility()
    if (fileContext.value?.id !== context?.id) void activateFiles()
  }
}
async function connectMissing(connectNew = true) {
  if (!record.value || launching.value) return
  const current = asLayout(record.value.layout)
  const sessions = await remoteApi.listSessions()
  const ids = connectNew && activeTab.value ? terminalIds({ ...current, tabs: [activeTab.value] }) : terminalIds(current)
  const { attach, connect } = partitionSessions(ids, sessions)
  for (const item of attach) {
    const entry = ensureEntry(item.configId)
    if (entry.client.id === item.terminalId) continue
    const channel = entry.client.createChannel()
    await terminalApi.subscribeTerminal(item.terminalId, channel)
    registry.bind(item.configId, entry, item.terminalId)
    const session = sessions.find((candidate) => candidate.terminalId === item.terminalId)
    if (session) entry.client.updateStatus(session.status)
  }
  const needSftp = !!fileContext.value && !isSftpReady(fileContext.value.id)
  sftpUp.value = !!fileContext.value && !needSftp
  if (!connectNew) return
  if (!connect.length && !needSftp) {
    if (fileContext.value) await browse(terminalDirectory.value || fileContext.value.lastDirectory || '.')
    return
  }
  await launch(connect, needSftp)
}
async function reload() {
  initializing.value = true
  reloadToken += 1
  const token = reloadToken
  browseGeneration += 1
  if (browseId) void remoteApi.cancelBrowse(browseId).catch(() => {})
  browseId = ''
  browsing.value = false
  disposeEntries()
  entries.value = []
  selected.value = []
  sftpError.value = ''
  try {
    const loaded = await remoteApi.getProjectWorkspace(props.workspaceId)
    if (token !== reloadToken) return
    record.value = loaded
    pathInput.value = '.'
    directory.value = pathInput.value
    const current = asLayout(loaded.layout)
    const tab = current.tabs.find((tab) => tab.id === current.activeTabId)
    focused.value = tab ? leaves(tab.root).find((leaf) => leaf.type === 'terminal')?.terminalConfigId ?? null : null
    await connectMissing(false)
  } catch (reason) { if (token === reloadToken) fail(reason) }
  finally { if (token === reloadToken) initializing.value = false }
}
let directorySaveChain: Promise<unknown> = Promise.resolve()
let pendingTerminalPath: string | null = null
let pendingTerminalConfig: string | null = null
let pendingTerminalSession: string | null = null
let injectedTerminalPath: string | null = null
async function browse(path: string, resolvePath = false, syncTerminal = false) {
  if (!fileContext.value || !isSftpReady(fileContext.value.id)) return
  const generation = ++browseGeneration
  const terminalConfig = syncTerminal ? focused.value : null
  const terminalSession = terminalConfig ? registry.byConfig.get(terminalConfig)?.client?.id ?? null : null
  if (browseId) void remoteApi.cancelBrowse(browseId).catch(() => {})
  browseId = ''
  directory.value = path
  pathInput.value = path
  browsing.value = true
  selected.value = []
  entries.value = []
  const workspaceId = fileContext.value.id
  const channel = new Channel<DirBatch>()
  channel.onmessage = (batch) => {
    if (generation !== browseGeneration || fileContext.value?.id !== workspaceId) return
    if (batch.entries.length) entries.value = [...entries.value, ...batch.entries]
    if (batch.path) {
      directory.value = batch.path
      pathInput.value = batch.path
    }
    if (!batch.done) return
    browsing.value = false
    sftpError.value = batch.message || ''
    if (batch.message) entries.value = []
    if (!batch.message && batch.path) {
      for (const context of record.value?.contexts ?? []) {
        if (context.connectionId === fileContext.value?.connectionId) context.lastDirectory = batch.path
      }
      directorySaveChain = directorySaveChain.catch(() => {}).then(() => {
        return remoteApi.setDirectory(workspaceId, batch.path)
      }).catch(fail)
      if (syncTerminal) queueTerminalDirectory(batch.path, terminalConfig, terminalSession)
    }
  }
  try {
    if (resolvePath) {
      path = await remoteApi.canonicalize(workspaceId, path)
      if (generation !== browseGeneration || fileContext.value?.id !== workspaceId) return
      directory.value = path
      pathInput.value = path
    }
    const requestId = await remoteApi.listDirectory(workspaceId, path, channel)
    if (generation !== browseGeneration) {
      void remoteApi.cancelBrowse(requestId).catch(() => {})
      return
    }
    browseId = requestId
  } catch (reason) {
    if (generation === browseGeneration) {
      browsing.value = false
      entries.value = []
      sftpError.value = fail(reason)
    }
  }
}
function followTerminalDirectory() {
  if (pendingTerminalPath && pendingTerminalConfig === focused.value) return
  const path = terminalDirectory.value
  if (props.active && sftpUp.value && path && path !== directory.value) void browse(path)
}
function queueTerminalDirectory(path: string, configId: string | null, sessionId: string | null) {
  if (!configId || !sessionId || focused.value !== configId || !path.startsWith('/')) return
  if (registry.byConfig.get(configId)?.client?.id !== sessionId) return
  pendingTerminalPath = path
  pendingTerminalConfig = configId
  pendingTerminalSession = sessionId
  injectedTerminalPath = null
  flushTerminalDirectory()
}
function clearTerminalDirectory() {
  pendingTerminalPath = null
  pendingTerminalConfig = null
  pendingTerminalSession = null
  injectedTerminalPath = null
}
function flushTerminalDirectory() {
  const path = pendingTerminalPath
  const configId = pendingTerminalConfig
  if (!path || !configId) return
  if (focused.value !== configId) {
    clearTerminalDirectory()
    return
  }
  const entry = registry.byConfig.get(configId)
  if (!entry || entry.client?.id !== pendingTerminalSession || entry.status?.state === 'exited' || entry.status?.state === 'failed') {
    clearTerminalDirectory()
    followTerminalDirectory()
    return
  }
  if (!entry.client?.id || entry.status?.state !== 'running') return
  if (entry.shell.cwd === path) {
    clearTerminalDirectory()
    return
  }
  if (injectedTerminalPath === path && entry.remotePrompt === 'ready' && entry.shell.cwd && entry.shell.cwd !== path) {
    clearTerminalDirectory()
    followTerminalDirectory()
    return
  }
  if (injectedTerminalPath === path || entry.remotePrompt !== 'ready') return
  const command = remoteCdCommand(path)
  if (!command) {
    clearTerminalDirectory()
    followTerminalDirectory()
    return
  }
  injectedTerminalPath = path
  entry.client.term.input(command, true)
}
watch(() => [focused.value, terminalDirectory.value, props.active], followTerminalDirectory)
watch(() => {
  const entry = focused.value ? registry.byConfig.get(focused.value) : undefined
  return [entry?.remotePrompt, entry?.shell?.cwd, entry?.client?.id, entry?.status?.state, focused.value]
}, flushTerminalDirectory)
function openParent() {
  const path = parent.value
  if (path) void browse(path, false, true)
}
function openEntry(entry: DirEntry) {
  if (entry.kind !== 'dir') return
  void browse(joinRemote(directory.value, entry.name), false, true)
}
function toggleSelect(name: string) {
  selected.value = selected.value.includes(name) ? selected.value.filter((item) => item !== name) : [...selected.value, name]
}
async function submitPath() {
  if (!fileContext.value) return
  try {
    await browse(pathInput.value, true, true)
  } catch (reason) { sftpError.value = fail(reason) }
}
async function makeDirectory() {
  if (!fileContext.value || !folderName.value.trim()) return
  const workspaceId = fileContext.value.id
  const path = directory.value
  busy.value = true
  try {
    await remoteApi.createDirectory(workspaceId, joinRemote(path, folderName.value.trim()))
    folderOpen.value = false
    folderName.value = ''
    if (fileContext.value?.id === workspaceId && directory.value === path) await browse(path)
  } catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function transfer(kind: 'upload-files' | 'upload-folders' | 'download', names: string[] = [], target?: { workspaceId: string; directory: string }) {
  if (!target && !fileContext.value) return
  if (kind === 'download' && !names.length) {
    emit('error', t('remote.selectDownload'))
    return
  }
  const downloadNames = target ? names : names.filter((name) => entries.value.find((entry) => entry.name === name)?.kind !== 'unsupported')
  if (kind === 'download' && !downloadNames.length) {
    emit('error', t('remote.kind.unsupported'))
    return
  }
  try {
    const workspaceId = target?.workspaceId ?? fileContext.value!.id
    const path = target?.directory ?? directory.value
    const grant = await remoteApi.selectPaths(workspaceId, kind)
    if (!grant) return
    await remoteApi.enqueueTransfer(workspaceId, grant.token, crypto.randomUUID(), path, kind === 'download' ? downloadNames : [])
  } catch (reason) { fail(reason) }
}
async function retry(transferItem: TransferView) {
  const target = { workspaceId: transferItem.workspaceId, directory: transferItem.remoteDirectory }
  if (transferItem.direction === 'download') await transfer('download', failedRoots(transferItem.files), target)
  else await transfer('upload-files', [], target)
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
  if (!pending || submittingSecret.value || !secret.value) return
  const value = secret.value
  secret.value = ''
  submittingSecret.value = true
  try {
    await remoteApi.submitCredential(pending.batchId, value, pending.authType === 'password' && savePassword.value)
    credential.value = null
    nextPrompts()
    savePassword.value = false
  } catch (reason) { fail(reason) }
  finally { submittingSecret.value = false }
}
async function cancelSecret() {
  if (submittingSecret.value) return
  savePassword.value = false
  const pending = credential.value
  secret.value = ''
  credential.value = null
  if (pending) await remoteApi.cancelAttempt(batchCancelId(pending.batchId)).catch((reason) => fail(reason))
  nextPrompts()
}
async function answer(decision: 'once' | 'save' | 'cancel') {
  const current = challenge.value
  challenge.value = null
  if (!current?.challengeId) return
  try { await remoteApi.answerChallenge(current.challengeId, decision) }
  catch (reason) { fail(reason) }
  finally { nextPrompts() }
}
function selectTab(id: string) {
  if (!record.value || busy.value) return
  if (record.value.layout.activeTabId === id) { void connectMissing().catch(fail); return }
  const next = { ...asLayout(record.value.layout), activeTabId: id }
  const tab = next.tabs.find((tab) => tab.id === id)
  focused.value = tab ? leaves(tab.root).find((leaf) => leaf.type === 'terminal')?.terminalConfigId ?? null : null
  void persist(next).catch(fail)
  void nextTick(() => { void connectMissing().catch(fail) })
}
function renameTab(id: string) {
  if (!props.active || props.shortcutsBlocked || dialogOpen.value || busy.value) return false
  const tab = layout.value?.tabs.find((item) => item.id === id)
  if (!tab) return false
  window.clearTimeout(ratioTimer)
  rename.value = { id: tab.id, title: tab.title }
  return true
}
function renameActiveTab() {
  const id = layout.value?.activeTabId
  return !!id && renameTab(id)
}
function renameFromPointer(event: MouseEvent, id: string) {
  if (renameTab(id)) event.preventDefault()
}
async function saveRename() {
  const target = rename.value
  const title = target?.title.trim()
  if (!target || !title || !record.value || savingRename.value || busy.value) return
  const next = { ...asLayout(record.value.layout), tabs: asLayout(record.value.layout).tabs.map((tab) => tab.id === target.id ? { ...tab, title } : tab) }
  savingRename.value = true
  busy.value = true
  window.clearTimeout(ratioTimer)
  try {
    const layoutValue = next as unknown as WorkspaceRecord['layout']
    await enqueueLayoutSave(record.value.id, layoutValue)
    if (record.value && rename.value?.id === target.id) {
      record.value = { ...record.value, layout: layoutValue }
      rename.value = null
    }
  } catch (reason) { fail(reason) }
  finally { savingRename.value = false; busy.value = false }
}
function cycleTab(delta: number) {
  const current = layout.value
  if (!current || current.tabs.length < 2) return
  const index = current.tabs.findIndex((tab) => tab.id === current.activeTabId)
  const tab = current.tabs[(index + delta + current.tabs.length) % current.tabs.length]
  if (!tab) return
  selectTab(tab.id)
  void nextTick(() => {
    if (props.active && !props.shortcutsBlocked && !dialogOpen.value && layout.value?.activeTabId === tab.id) focusTerminal()
  })
}
function keydown(event: KeyboardEvent) {
  if (!props.active || props.shortcutsBlocked || dialogOpen.value || busy.value) return
  if (Array.from(document.querySelectorAll('[aria-modal="true"]')).some((element) => element.getClientRects().length > 0)) return
  const target = event.target as HTMLElement | null
  if (target && !target.closest?.('.xterm') && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) return
  const command = matchShortcut(event, props.settings.shortcuts, ['workspace', 'remote'])
  if (command && /^tab[1-9]$/.test(command)) {
    const tab = layout.value?.tabs[Number(command.slice(3)) - 1]
    if (!tab) return
    event.preventDefault()
    event.stopPropagation()
    if (!event.repeat) { selectTab(tab.id); void nextTick(focusTerminal) }
    return
  }
  if (command !== 'previousTab' && command !== 'nextTab' && command !== 'remotePrevious' && command !== 'remoteNext' && command !== 'newTerminal' && command !== 'splitHorizontal' && command !== 'splitVertical') return
  event.preventDefault()
  event.stopPropagation()
  if (event.repeat) return
  if (command === 'newTerminal') void addTerminal()
  else if (command === 'splitHorizontal' || command === 'splitVertical') void addTerminal(command === 'splitHorizontal' ? 'horizontal' : 'vertical')
  else cycleTab(command === 'previousTab' || command === 'remotePrevious' ? -1 : 1)
}
function askClose(ids: string[], title: string, tabId?: string) {
  if (!ids.length) return
  closing.value = { ids, title, tabId }
  // The local "confirm before closing panes" setting does not skip this prompt.
  if (!remotePaneNeedsCloseConfirm()) void confirmClose()
}
function closeTab(tab: Layout['tabs'][number]) {
  if (busy.value) return
  askClose(leaves(tab.root).flatMap((leaf) => leaf.type === 'terminal' ? [leaf.terminalConfigId] : []), t('remote.closeTab', { title: tab.title }), tab.id)
}
async function confirmClose() {
  const request = closing.value
  if (!request || !record.value || busy.value) return
  busy.value = true
  try {
    window.clearTimeout(ratioTimer)
    await enqueueLayoutSave(record.value.id, record.value.layout)
    await remoteApi.removeProjectTerminals(record.value.id, request.ids)
    for (const id of request.ids) registry.remove(id)
    record.value = await remoteApi.getProjectWorkspace(record.value.id)
    focused.value = activeTab.value ? leaves(activeTab.value.root).flatMap((leaf) => leaf.type === 'terminal' ? [leaf.terminalConfigId] : [])[0] ?? null : null
    closing.value = null
  } catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function addTerminal(direction?: 'horizontal' | 'vertical') {
  if (!record.value || busy.value || dialogOpen.value || launching.value) return
  if (direction && !focused.value) return
  busy.value = true
  try {
    await remote.reload()
    creationTarget = { direction, tabId: layout.value?.activeTabId ?? null, focused: focused.value }
    chosen.value = null
    choosing.value = true
  } catch (reason) { fail(reason) }
  finally { busy.value = false }
}
async function createChosenTerminal() {
  const target = creationTarget
  const selectedConnection = remote.connections.find((connection) => connection.id === chosen.value)
  if (!target || !selectedConnection || !record.value || creating.value || busy.value || launching.value) return
  creating.value = true
  busy.value = true
  let config: ProjectTerminal | null = null
  const previous = record.value
  try {
    config = await remoteApi.createProjectTerminal(previous.id, selectedConnection.id, selectedConnection.name)
    const loaded = await remoteApi.getProjectWorkspace(previous.id)
    const current = JSON.parse(JSON.stringify(asLayout(previous.layout))) as Layout
    const tab = current.tabs.find((item) => item.id === target.tabId)
    if (target.direction && tab && target.focused) {
      tab.root = splitLeaf(tab.root, target.focused, { type: 'terminal', terminalConfigId: config.id }, target.direction)
    } else {
      const tabId = crypto.randomUUID()
      current.tabs.push({ id: tabId, title: selectedConnection.name, root: { type: 'terminal', terminalConfigId: config.id } })
      current.activeTabId = tabId
    }
    await enqueueLayoutSave(previous.id, current as unknown as WorkspaceRecord['layout'])
    record.value = { ...loaded, layout: current as unknown as WorkspaceRecord['layout'] }
    focused.value = config.id
    choosing.value = false
  } catch (reason) {
    if (config) await remoteApi.removeTerminal(config.remoteWorkspaceId, config.id).catch(fail)
    fail(reason)
    config = null
  } finally { busy.value = false; creating.value = false }
  if (config) await launch([config.id], !!fileContext.value && !isSftpReady(fileContext.value.id))
}
async function activateFiles(connect = true) {
  browseGeneration += 1
  if (browseId) void remoteApi.cancelBrowse(browseId).catch(() => {})
  browseId = ''
  entries.value = []
  selected.value = []
  browsing.value = false
  sftpError.value = ''
  clearTerminalDirectory()
  const context = fileContext.value
  directory.value = context?.lastDirectory || '.'
  pathInput.value = directory.value
  sftpUp.value = !!context && isSftpReady(context.id)
  if (!context || !props.active || initializing.value || busy.value || launching.value || !focused.value || !registry.byConfig.has(focused.value)) return
  if (sftpUp.value) await browse(terminalDirectory.value || directory.value)
  else if (connect) await launch([], true)
}
async function refreshConnections() {
  try { await remote.reload() } catch (reason) { fail(reason) }
}
async function refreshProject() {
  if (!record.value || initializing.value || busy.value || launching.value || refreshing.value) return
  refreshPending.value = false
  refreshing.value = true
  busy.value = true
  try {
    window.clearTimeout(ratioTimer)
    await enqueueLayoutSave(record.value.id, record.value.layout).catch(() => {})
    await saveChain.catch(() => {})
    const loaded = await remoteApi.getProjectWorkspace(props.workspaceId)
    const ids = new Set(loaded.terminals.map((terminal) => terminal.id))
    for (const id of registry.byConfig.keys()) if (!ids.has(id)) registry.remove(id)
    record.value = loaded
    if (!focused.value || !ids.has(focused.value)) focused.value = activeTab.value ? leaves(activeTab.value.root).find((leaf) => leaf.type === 'terminal')?.terminalConfigId ?? null : null
    await connectMissing(false)
  } catch (reason) { fail(reason) }
  finally { refreshing.value = false; busy.value = false; void activateFiles(false) }
}
function onRatio(path: number[], value: number) {
  if (!record.value || busy.value || rename.value) return
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
const sessionTimer = window.setInterval(() => { void syncSessions() }, 2000)
watch(rename, (value) => { if (value) void nextTick(() => { renameInput.value?.focus(); renameInput.value?.select() }) })
watch(() => fileContext.value?.id, () => { void activateFiles() })
watch(() => remote.revision, () => { refreshPending.value = true })
watch(() => [refreshPending.value, busy.value, launching.value, initializing.value, refreshing.value], () => {
  if (refreshPending.value) void refreshProject()
})
watch(() => record.value?.terminals.length, (count) => { remote.terminalCounts.set(props.workspaceId, count ?? 0) })
watch(() => props.workspaceId, () => { void reload() }, { immediate: true })
watch(() => props.active, (active) => { if (active && !initializing.value) void activateFiles() })
watch(() => [props.active, record.value?.layout.activeTabId, record.value?.layout.tabs.length], syncVisibility, { flush: 'post' })
watch(() => props.settings, (settings) => registry.updateSettings(settings), { deep: true })
watch(() => props.appearance, () => registry.updateSettings(props.settings))
onMounted(() => { document.addEventListener('keydown', keydown, true); void remote.listenTransfers().catch(fail) })
onBeforeUnmount(() => {
  document.removeEventListener('keydown', keydown, true)
  window.clearInterval(sessionTimer)
  window.clearTimeout(ratioTimer)
  reloadToken += 1
  browseGeneration += 1
  if (browseId) void remoteApi.cancelBrowse(browseId)
  disposeEntries()
})
function focusTerminal() { if (focused.value) registry.byConfig.get(focused.value)?.client.focus() }
defineExpose({ connectMissing, focusTerminal, renameActiveTab })
</script>
<template>
  <section v-if="record && layout" class="remote-workspace">
    <div class="remote-toolbar">
      <span class="remote-title">{{ connection?.name }}</span>
      <span class="remote-connection-endpoint">{{ endpoint }}</span>
      <NButton size="small" :disabled="launching || !focused" @click="focused && launch([focused], false)">{{ t('remote.reconnect') }}</NButton>
      <NButton size="small" @click="addTerminal()">{{ t('remote.newTab') }}</NButton>
      <NButton size="small" :disabled="!focused" @click="addTerminal('horizontal')">{{ t('remote.splitHorizontal') }}</NButton>
      <NButton size="small" :disabled="!focused" @click="addTerminal('vertical')">{{ t('remote.splitVertical') }}</NButton>
    </div>
    <Teleport v-if="active" to="#remote-titlebar-tabs">
      <div class="workspace-tabs" role="tablist" :aria-label="t('workspace.tabs')">
        <div v-for="(tab, index) in layout.tabs" :key="tab.id" class="workspace-tab" :class="{ active: tab.id === layout.activeTabId }" @mousedown.middle.prevent @auxclick.middle.prevent.stop="closeTab(tab)">
          <button role="tab" :aria-selected="tab.id === layout.activeTabId" :title="index < 9 ? hint(`tab${index + 1}`) : undefined" @click="selectTab(tab.id)" @dblclick="renameTab(tab.id)" @contextmenu="renameFromPointer($event, tab.id)">{{ tab.title }}</button>
          <button :aria-label="t('remote.closeTab', { title: tab.title })" :disabled="busy" @click="closeTab(tab)">×</button>
        </div>
        <button class="workspace-tab-add" :disabled="busy || launching" :title="t('remote.newTab')" :aria-label="t('remote.newTab')" @click="addTerminal()">+</button>
      </div>
    </Teleport>
    <div v-if="!activeTab" class="workspace-placeholder">
      <p>{{ t('remote.projectPlaceholder') }}</p>
      <NButton @click="addTerminal()">{{ t('remote.newTab') }}</NButton>
    </div>
    <div v-else class="remote-body">
      <div class="remote-terminals">
        <RemoteLayoutPane v-if="activeTab" :node="activeTab.root" :path="[]" :registry="registry" :names="names" :focused="focused" :visible="active" :endpoints="endpoints" @focus="focused = $event" @close="askClose([$event], t('remote.closePane'))" @reconnect="(id) => launch([id], false)" @ratio="onRatio" />
      </div>
      <aside class="remote-files">
        <div class="remote-file-toolbar">
          <strong :title="endpoint">{{ t('remote.files') }} · {{ connection?.name }}</strong>
          <NButton size="tiny" :disabled="browsing || !sftpUp" @click="browse(directory)">{{ t('remote.refresh') }}</NButton>
          <NButton size="tiny" :disabled="!browsing" @click="browseId && remoteApi.cancelBrowse(browseId)">{{ t('remote.cancelBrowse') }}</NButton>
        </div>
        <div class="remote-current-path" :title="directory">{{ t('remote.path') }}: {{ directory }}</div>
        <div v-if="terminalDirectory && terminalDirectory !== directory" class="remote-current-path" :title="terminalDirectory">{{ t('remote.terminalPath') }}: {{ terminalDirectory }}</div>
        <div class="remote-path">
          <NButton size="tiny" :disabled="!parent" @click="openParent">{{ t('remote.parent') }}</NButton>
          <NInput v-model:value="pathInput" :title="directory" :placeholder="t('remote.path')" @keydown.enter="submitPath" />
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
            <div>{{ remote.connections.find((connection) => connection.id === record?.contexts.find((context) => context.id === item.workspaceId)?.connectionId)?.name }} · {{ t(directionText[item.direction] ?? 'remote.direction.upload') }} · {{ progress(item) }}</div>
            <div v-if="item.message">{{ item.message }}</div>
            <NButton size="tiny" @click="remoteApi.cancelTransfer(item.id).catch(fail)">{{ t('remote.cancelTransfer') }}</NButton>
            <NButton v-if="item.state === 'failed' || item.state === 'cancelled' || item.files.some((file) => file.status === 'failed')" size="tiny" @click="retry(item)">{{ t('remote.retryTransfer') }}</NButton>
          </div>
        </div>
      </aside>
    </div>
    <NModal :show="active && choosing" preset="card" :title="t('remote.newTab')" class="form-modal" :mask-closable="!creating" @update:show="(show) => { if (!creating) choosing = show }">
      <p>{{ t('remote.chooseConnection') }}</p>
      <NSelect v-model:value="chosen" :options="connectionOptions" filterable :disabled="creating" />
      <div class="dialog-actions">
        <NButton :disabled="creating" @click="manager = true">{{ t('remote.manageConnections') }}</NButton>
        <NButton :disabled="creating" @click="choosing = false">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :loading="creating" :disabled="!chosen || creating || busy || launching" @click="createChosenTerminal">{{ t('remote.connect') }}</NButton>
      </div>
    </NModal>
    <NModal :show="active && manager" preset="card" :title="t('remote.manageConnections')" class="form-modal" @update:show="manager = $event">
      <ConnectionManager @changed="refreshConnections" @error="emit('error', $event)" />
    </NModal>
    <NModal :show="active && !!rename" preset="card" :title="t('workspace.rename')" class="form-modal" :mask-closable="!savingRename" @update:show="(show) => { if (!show && !savingRename) rename = null }">
      <NInput v-if="rename" ref="renameInput" v-model:value="rename.title" :maxlength="100" :disabled="savingRename" :placeholder="t('workspace.tabTitle')" @keydown.enter.prevent="saveRename" />
      <div class="dialog-actions">
        <NButton :disabled="savingRename" @click="rename = null">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :loading="savingRename" :disabled="savingRename || !rename?.title.trim()" @click="saveRename">{{ t('common.save') }}</NButton>
      </div>
    </NModal>
    <NModal :show="active && !!credential" preset="card" :title="t('remote.credentialTitle')" class="form-modal" :mask-closable="false" @update:show="(show) => { if (!show) void cancelSecret() }">
      <p>{{ t(credential?.authType === 'privateKey' ? 'remote.passphrasePrompt' : 'remote.passwordPrompt', { endpoint: credentialEndpoint }) }}</p>
      <NInput v-model:value="secret" type="password" show-password-on="click" :placeholder="t('remote.secret')" :disabled="submittingSecret" @keydown.enter="submitSecret" />
      <NCheckbox v-if="credential?.authType === 'password'" v-model:checked="savePassword" :disabled="submittingSecret">{{ t('remote.savePassword') }}</NCheckbox>
      <div class="dialog-actions">
        <NButton @click="cancelSecret">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :disabled="!secret || submittingSecret" :loading="submittingSecret" @click="submitSecret">{{ t('common.confirm') }}</NButton>
      </div>
    </NModal>
    <NModal :show="active && !!challenge" preset="card" :title="t('remote.challengeTitle')" class="form-modal" :mask-closable="false" @update:show="(show) => { if (!show) void answer('cancel') }">
      <p>{{ t('remote.challengeBody', { host: challenge?.host ?? '', port: challenge?.port ?? '' }) }}</p>
      <p>{{ t('remote.algorithm') }}: {{ challenge?.algorithm }}</p>
      <p>{{ t('remote.fingerprint') }}: {{ challenge?.fingerprint }}</p>
      <div class="dialog-actions">
        <NButton @click="answer('cancel')">{{ t('common.cancel') }}</NButton>
        <NButton @click="answer('once')">{{ t('remote.trustOnce') }}</NButton>
        <NButton type="primary" @click="answer('save')">{{ t('remote.trustSave') }}</NButton>
      </div>
    </NModal>
    <NModal :show="active && !!closing" preset="card" :title="t('workspace.confirmClose')" class="form-modal" :mask-closable="false" @update:show="(show) => { if (!show && !busy) closing = null }">
      <p>{{ closing?.title }}</p>
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="closing = null">{{ t('common.cancel') }}</NButton>
        <NButton type="error" :loading="busy" @click="confirmClose">{{ t('common.confirm') }}</NButton>
      </div>
    </NModal>
    <NModal :show="active && folderOpen" @update:show="folderOpen = $event" preset="card" :title="t('remote.mkdir')" class="form-modal">
      <NInput v-model:value="folderName" :placeholder="t('remote.folderName')" @keydown.enter="makeDirectory" />
      <div class="dialog-actions">
        <NButton @click="folderOpen = false">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :loading="busy" :disabled="!folderName.trim()" @click="makeDirectory">{{ t('common.confirm') }}</NButton>
      </div>
    </NModal>
    <NModal :show="active && !!conflictTransfer" preset="card" :title="t('remote.conflictTitle')" class="form-modal" :mask-closable="false">
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
