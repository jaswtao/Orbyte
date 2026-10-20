<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { NButton, NInput, NModal } from 'naive-ui'
import { fileApi, fileError, type DirectoryEntry, type FileChange } from '../../services/fileApi'
import { useGit } from '../../stores/git'
import type { GitKind } from '../../services/gitApi'
import { t } from '../../i18n'
const git = useGit()
const props = defineProps<{ projectId: string; change: FileChange | null; disabled: boolean }>()
function parents(path: string) {
  const result: string[] = []
  for (let i = path.lastIndexOf('/'); i > 0; i = path.lastIndexOf('/', i - 1)) result.push(path.slice(0, i))
  return result
}
const gitIndex = computed(() => {
  const exact = new Map<string, GitKind>()
  const changedDirs = new Set<string>()
  const untrackedDirs = new Set<string>()
  for (const file of git.statuses.get(props.projectId)?.files ?? []) {
    // Git 用 `dir/` 表示整个未跟踪目录。
    const path = file.path.toLowerCase().replace(/\/$/, '')
    exact.set(path, file.kind)
    if (file.kind === 'untracked' && file.path.endsWith('/')) untrackedDirs.add(path)
    for (const parent of parents(path)) changedDirs.add(parent)
  }
  return { exact, changedDirs, untrackedDirs }
})
function gitClass(entry: DirectoryEntry) {
  const { exact, changedDirs, untrackedDirs } = gitIndex.value
  const path = entry.path.toLowerCase()
  const kind = exact.get(path)
    ?? (untrackedDirs.size && parents(path).some((parent) => untrackedDirs.has(parent)) ? 'untracked' : undefined)
    ?? (entry.isDirectory && changedDirs.has(path) ? 'modified' : undefined)
  return kind ? `git-${kind}` : null
}
const emit = defineEmits<{ open: [path: string]; rename: [from: string, to: string]; error: [message: string] }>()
const directories = reactive(new Map<string, { entries: DirectoryEntry[]; truncated: boolean }>())
const expanded = reactive(new Set(['']))
const selected = ref<DirectoryEntry | null>(null)
const menu = ref(false)
const dialog = ref<{ operation: 'file' | 'directory' | 'rename' | 'delete'; path: string; original: string } | null>(null)
const busy = ref(false)
const rows = computed(() => {
  const result: (DirectoryEntry & { depth: number; truncated?: boolean })[] = []
  const visit = (path: string, depth: number) => {
    for (const entry of directories.get(path)?.entries ?? []) { result.push({ ...entry, depth }); if (entry.isDirectory && expanded.has(entry.path)) visit(entry.path, depth + 1) }
    if (directories.get(path)?.truncated) result.push({ name: t('files.truncated'), path: `${path}/__truncated`, isDirectory: false, accessible: false, depth, truncated: true })
  }
  visit('', 0); return result
})
async function load(path: string) {
  try { directories.set(path, await fileApi.list(props.projectId, path)) }
  catch (reason) {
    const error = fileError(reason)
    if (error.kind === 'missing' && path) { directories.delete(path); expanded.delete(path) }
    else emit('error', error.message)
  }
}
async function toggle(entry: DirectoryEntry) {
  if (props.disabled) return
  selected.value = entry; menu.value = false
  if (!entry.accessible) return
  if (!entry.isDirectory) { emit('open', entry.path); return }
  if (expanded.has(entry.path)) expanded.delete(entry.path)
  else { expanded.add(entry.path); await load(entry.path) }
}
function action(operation: 'file' | 'directory' | 'rename' | 'delete') {
  if (props.disabled) return
  menu.value = false
  const original = selected.value?.path ?? ''
  const parent = selected.value?.isDirectory ? original : original.split('/').slice(0, -1).join('/')
  dialog.value = { operation, original, path: operation === 'rename' || operation === 'delete' ? original : parent ? `${parent}/` : '' }
}
async function execute() {
  if (!dialog.value || busy.value || props.disabled) return
  busy.value = true
  const { operation, path, original } = dialog.value
  try {
    if (operation === 'rename') { await fileApi.rename(props.projectId, original, path); emit('rename', original, path); selected.value = null; directories.clear(); expanded.clear(); expanded.add('') }
    else if (operation === 'delete') { await fileApi.delete(props.projectId, original); selected.value = null }
    else await fileApi.create(props.projectId, path, operation === 'directory')
    dialog.value = null
    await refresh()
    if (operation === 'file') emit('open', path)
  } catch (reason) { emit('error', fileError(reason).message) } finally { busy.value = false }
}
async function refresh() { await Promise.all(Array.from(expanded, load)) }
watch(() => props.change, (change) => {
  if (!change || 'rescan' in change) { void refresh(); return }
  const affected = Array.from(expanded).filter((directory) => change.paths.some((path) => !path || path.slice(0, path.lastIndexOf('/') < 0 ? 0 : path.lastIndexOf('/')).toLowerCase() === directory.toLowerCase() || directory.toLowerCase() === path.toLowerCase() || directory.toLowerCase().startsWith(`${path.toLowerCase()}/`)))
  void Promise.all(affected.map(load))
})
onMounted(() => { void load('') })
</script>
<template>
  <div class="file-tree" @contextmenu.prevent="menu = true">
    <div class="file-actions"><NButton size="tiny" :disabled="disabled" @click="action('file')">{{ t('files.newFile') }}</NButton><NButton size="tiny" :disabled="disabled" @click="action('directory')">{{ t('files.newDirectory') }}</NButton><NButton size="tiny" @click="refresh">{{ t('files.refresh') }}</NButton></div>
    <p class="sidebar-tip">{{ t('files.hint') }}</p>
    <div v-if="menu" class="file-actions"><NButton size="tiny" :disabled="!selected?.accessible" @click="action('rename')">{{ t('files.rename') }}</NButton><NButton size="tiny" :disabled="!selected?.accessible" @click="action('delete')">{{ t('files.trash') }}</NButton><NButton size="tiny" @click="menu = false">{{ t('common.cancel') }}</NButton></div>
    <div class="file-rows"><button v-for="entry in rows" :key="entry.path" :style="{ paddingLeft: `${8 + entry.depth * 16}px` }" :class="[{ selected: selected?.path === entry.path }, gitClass(entry)]" :title="entry.accessible ? entry.path : t('files.inaccessible')" @click="toggle(entry)" @contextmenu.prevent.stop="selected = entry; menu = true">{{ entry.isDirectory ? expanded.has(entry.path) ? '▾' : '▸' : '·' }} {{ entry.name }}{{ !entry.accessible && !entry.truncated ? ' 🔒' : '' }}</button></div>
    <NModal :show="!!dialog" preset="card" :title="dialog?.operation === 'delete' ? t('files.confirmTrash') : dialog?.operation === 'rename' ? t('files.rename') : t('files.createTitle')" class="form-modal" @update:show="(show) => { if (!show && !busy) dialog = null }"><template v-if="dialog"><p v-if="dialog.operation === 'delete'">{{ t('files.confirmTrashBody', { name: dialog.original }) }}</p><NInput v-else v-model:value="dialog.path" :placeholder="t('files.pathPlaceholder')" /><div class="dialog-actions"><NButton :disabled="busy" @click="dialog = null">{{ t('common.cancel') }}</NButton><NButton :loading="busy" :disabled="!dialog.path" @click="execute">{{ t('common.confirm') }}</NButton></div></template></NModal>
  </div>
</template>
