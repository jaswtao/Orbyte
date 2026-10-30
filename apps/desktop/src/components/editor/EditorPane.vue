<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { NButton, NDropdown, NModal, type DropdownOption } from 'naive-ui'
import { redo, redoDepth, selectAll, undo, undoDepth } from '@codemirror/commands'
import { openSearchPanel } from '@codemirror/search'
import { Transaction } from '@codemirror/state'
import type { EditorRegistry } from '../../services/editor/registry'
import type { Encoding } from '../../services/fileApi'
import { invoke } from '../../services/diagnostics'
import { showWorkspaceMessage } from '../../services/workspace/messages'
import { t } from '../../i18n'
const props = defineProps<{ id: string; editors: EditorRegistry; visible: boolean; focused: boolean }>()
const emit = defineEmits<{ focus: [id: string]; close: [id: string] }>()
const mount = ref<HTMLElement>()
const entry = computed(() => props.editors.entries.get(props.id))
const menu = ref<{ x: number; y: number } | null>(null)
const clipboardPending = ref(false)
let disposed = false
let contextVersion = 0
function closeMenu() { menu.value = null }
function menuOptions(): DropdownOption[] {
  const target = entry.value
  if (!target) return []
  const state = target.view.state
  const writable = !state.readOnly && !target.busy && target.initialized && !target.recovery
  const selected = state.selection.ranges.some((range) => !range.empty)
  return [
    { key: 'undo', label: t('editor.undo'), disabled: !writable || undoDepth(state) === 0 },
    { key: 'redo', label: t('editor.redo'), disabled: !writable || redoDepth(state) === 0 },
    { type: 'divider', key: 'history-divider' },
    { key: 'cut', label: t('editor.cut'), disabled: !writable || !selected || clipboardPending.value },
    { key: 'copy', label: t('editor.copy'), disabled: !selected || clipboardPending.value },
    { key: 'paste', label: t('editor.paste'), disabled: !writable || clipboardPending.value },
    { type: 'divider', key: 'clipboard-divider' },
    { key: 'selectAll', label: t('editor.selectAll') },
    { type: 'divider', key: 'selection-divider' },
    { key: 'find', label: t('editor.find') },
    { key: 'replace', label: t('editor.replace'), disabled: !writable },
    { type: 'divider', key: 'search-divider' },
    { key: 'save', label: t('editor.save'), disabled: target.busy || !target.metadata || !!target.recovery || !target.initialized },
  ]
}
function isPanelEvent(event: MouseEvent) {
  return event.target instanceof Element && !!event.target.closest('.cm-panels')
}
function openMenu(event: MouseEvent) {
  if (isPanelEvent(event)) { closeMenu(); return }
  event.preventDefault()
  event.stopPropagation()
  const target = entry.value
  if (!target || !props.visible) return
  focus()
  const view = target.view
  if (view.state.selection.ranges.every((range) => range.empty)) {
    const position = view.posAtCoords({ x: event.clientX, y: event.clientY })
    if (position !== null) view.dispatch({ selection: { anchor: position } })
  }
  view.focus()
  menu.value = { x: event.clientX, y: event.clientY }
}
function preserveSelection(event: MouseEvent) {
  if (event.button === 2 && !isPanelEvent(event)) event.preventDefault()
}
function escapeMenu(event: KeyboardEvent) {
  if (!menu.value || event.key !== 'Escape') return
  event.preventDefault()
  event.stopPropagation()
  closeMenu()
  entry.value?.view.focus()
}
async function selectMenu(key: string | number) {
  const target = entry.value
  const option = menuOptions().find((item) => item.key === key)
  closeMenu()
  if (!target || !props.visible || !option || option.disabled) return
  const view = target.view
  view.focus()
  if (key === 'undo') undo(view)
  else if (key === 'redo') redo(view)
  else if (key === 'selectAll') selectAll(view)
  else if (key === 'find' || key === 'replace') {
    openSearchPanel(view)
    if (key === 'replace') view.dom.querySelector<HTMLInputElement>('.cm-search [name="replace"]')?.focus()
  } else if (key === 'save') await props.editors.save(props.id)
  else if (key === 'copy' || key === 'cut' || key === 'paste') {
    const state = view.state
    const version = contextVersion
    const current = () => !disposed && props.visible && entry.value === target && contextVersion === version && view.state === state
    clipboardPending.value = true
    try {
      if (key === 'paste') {
        const text = await invoke<string | null>('read_clipboard_text')
        if (current() && !view.state.readOnly && !target.busy && text) {
          view.dispatch(state.update(state.replaceSelection(text), { scrollIntoView: true, annotations: Transaction.userEvent.of('input.paste') }))
        }
      } else {
        const text = state.selection.ranges.map(({ from, to }) => state.sliceDoc(from, to)).join(state.lineBreak)
        await invoke('write_clipboard_text', { text })
        if (key === 'cut' && current() && !view.state.readOnly && !target.busy) {
          view.dispatch(state.update(state.replaceSelection(''), { scrollIntoView: true, annotations: Transaction.userEvent.of('delete.cut') }))
        }
      }
    } catch (reason) {
      showWorkspaceMessage(t('editor.clipboardFailed', { reason: String(reason) }))
    } finally { clipboardPending.value = false }
  }
}
function attach() { if (mount.value && entry.value) { mount.value.appendChild(entry.value.view.dom); if (props.visible) entry.value.view.requestMeasure() } }
async function copyDraft() { try { await invoke('write_clipboard_text', { text: entry.value?.recovery?.content ?? entry.value?.document.content ?? '' }) } catch { if (entry.value) entry.value.draftError = t('draft.failed') } }
function focus() { emit('focus', props.id) }
function answer(action: 'cancel' | 'reload' | 'overwrite' | 'utf8') {
  const target = entry.value
  if (!target) return
  const dialog = target.dialog
  target.dialog = null
  if (action === 'reload') void props.editors.reload(props.id, dialog?.kind === 'reopen' ? dialog.encoding : undefined)
  if (action === 'overwrite') void props.editors.save(props.id, true)
  if (action === 'utf8') void props.editors.save(props.id, false, 'utf-8')
}
onMounted(() => { attach(); document.addEventListener('keydown', escapeMenu, true) })
onBeforeUnmount(() => { disposed = true; closeMenu(); document.removeEventListener('keydown', escapeMenu, true) })
watch(() => [props.id, props.visible], () => { contextVersion++; closeMenu() }, { flush: 'sync' })
watch(() => props.focused, (focused) => { if (!focused) { contextVersion++; closeMenu() } }, { flush: 'sync' })
watch(() => [props.id, props.visible], attach, { flush: 'post' })
</script>
<template>
  <div v-if="entry" class="terminal-pane editor-pane" :class="{ focused }" @focusin="focus" @mousedown="focus">
    <div class="pane-heading"><span :title="entry.path">{{ entry.document.dirty ? '● ' : '' }}{{ entry.path }}</span><button :disabled="entry.busy || !entry.metadata || !!entry.recovery || !entry.initialized" @click="editors.save(id)">{{ t('editor.save') }}</button><button :aria-label="t('editor.close')" @click="emit('close', id)">×</button></div>
    <div v-if="entry.recovery" class="terminal-notice">
      {{ editors.rootChanged(entry) ? t('draft.rootChanged') : t('draft.available') }}
      <button :disabled="entry.busy || editors.rootChanged(entry)" @click="editors.recover(id, true).catch(() => { entry!.draftError = t('draft.failed') })">{{ t('draft.restore') }}</button>
      <button @click="copyDraft">{{ t('draft.copy') }}</button>
      <button :disabled="entry.busy" @click="editors.recover(id, false).catch(() => { entry!.draftError = t('draft.failed') })">{{ t('draft.discard') }}</button>
    </div>
    <div v-if="entry.draftError" class="terminal-notice">{{ entry.draftError }} <button @click="entry.draftSync ? entry.draftSync.flush().catch(() => {}) : editors.reload(id)">{{ t('common.retry') }}</button></div>
    <div v-if="entry.document.external" class="terminal-notice">{{ t('editor.external') }} <button @click="entry.dialog = { kind: 'reopen', encoding: entry.metadata?.encoding ?? 'utf-8', message: t('editor.reloadExternal') }">{{ t('editor.reload') }}</button><button @click="entry.document.external = false">{{ t('editor.keepMine') }}</button></div>
    <div v-if="entry.deleted" class="terminal-notice">{{ t('editor.deleted') }}</div>
    <div v-if="entry.error" class="terminal-notice">{{ entry.error }} <button :disabled="entry.busy || !!entry.recovery" @click="entry.document.dirty ? editors.reopen(id, entry.metadata?.encoding ?? 'utf-8') : editors.reload(id)">{{ t('editor.reread') }}</button></div>
    <div ref="mount" class="editor-mount" @mousedown.capture="preserveSelection" @contextmenu="openMenu" @scroll.capture="closeMenu" />
    <NDropdown trigger="manual" placement="bottom-start" :show="!!menu" :x="menu?.x ?? 0" :y="menu?.y ?? 0" :options="menu ? menuOptions() : []" @select="selectMenu" @clickoutside="closeMenu" />
    <div class="pane-status"><select :value="entry.metadata?.encoding ?? 'utf-8'" :disabled="entry.busy || !!entry.recovery" :aria-label="t('editor.reopenEncoding')" @change="editors.reopen(id, ($event.target as HTMLSelectElement).value as Encoding)"><option value="utf-8">{{ t('editor.reopenUtf8') }}</option><option value="gbk">{{ t('editor.reopenGbk') }}</option></select> · {{ entry.metadata?.lineEnding }}{{ entry.metadata?.bom ? ' · BOM' : '' }}{{ entry.busy ? ` · ${t('editor.busy')}` : '' }}</div>
    <NModal :show="!!entry.dialog" preset="card" :title="t('editor.dialogTitle')" class="form-modal" @update:show="(show) => { if (!show) answer('cancel') }">
      <p>{{ entry.dialog?.message }}</p><div class="dialog-actions"><NButton @click="answer('cancel')">{{ t('common.cancel') }}</NButton><template v-if="entry.dialog?.kind === 'conflict'"><NButton @click="answer('reload')">{{ t('editor.reload') }}</NButton><NButton type="warning" @click="answer('overwrite')">{{ t('editor.overwrite') }}</NButton></template><NButton v-else-if="entry.dialog?.kind === 'unmappable'" @click="answer('utf8')">{{ t('editor.saveUtf8') }}</NButton><NButton v-else @click="answer('reload')">{{ t('editor.discardReopen') }}</NButton></div>
    </NModal>
  </div>
</template>
