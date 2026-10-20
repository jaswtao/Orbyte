<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { NButton, NModal } from 'naive-ui'
import type { EditorRegistry } from '../../services/editor/registry'
import type { Encoding } from '../../services/fileApi'
import { t } from '../../i18n'
const props = defineProps<{ id: string; editors: EditorRegistry; visible: boolean; focused: boolean }>()
const emit = defineEmits<{ focus: [id: string]; close: [id: string] }>()
const mount = ref<HTMLElement>()
const entry = computed(() => props.editors.entries.get(props.id))
function attach() { if (mount.value && entry.value) { mount.value.appendChild(entry.value.view.dom); if (props.visible) entry.value.view.requestMeasure() } }
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
onMounted(attach)
watch(() => [props.id, props.visible], attach, { flush: 'post' })
</script>
<template>
  <div v-if="entry" class="terminal-pane editor-pane" :class="{ focused }" @focusin="focus" @mousedown="focus">
    <div class="pane-heading"><span :title="entry.path">{{ entry.document.dirty ? '● ' : '' }}{{ entry.path }}</span><button :disabled="entry.busy || !entry.metadata" @click="editors.save(id)">{{ t('editor.save') }}</button><button :aria-label="t('editor.close')" @click="emit('close', id)">×</button></div>
    <div v-if="entry.document.external" class="terminal-notice">{{ t('editor.external') }} <button @click="entry.dialog = { kind: 'reopen', encoding: entry.metadata?.encoding ?? 'utf-8', message: t('editor.reloadExternal') }">{{ t('editor.reload') }}</button><button @click="entry.document.external = false">{{ t('editor.keepMine') }}</button></div>
    <div v-if="entry.deleted" class="terminal-notice">{{ t('editor.deleted') }}</div>
    <div v-if="entry.error" class="terminal-notice">{{ entry.error }} <button :disabled="entry.busy" @click="entry.document.dirty ? editors.reopen(id, entry.metadata?.encoding ?? 'utf-8') : editors.reload(id)">{{ t('editor.reread') }}</button></div>
    <div ref="mount" class="editor-mount" />
    <div class="pane-status"><select :value="entry.metadata?.encoding ?? 'utf-8'" :disabled="entry.busy" :aria-label="t('editor.reopenEncoding')" @change="editors.reopen(id, ($event.target as HTMLSelectElement).value as Encoding)"><option value="utf-8">{{ t('editor.reopenUtf8') }}</option><option value="gbk">{{ t('editor.reopenGbk') }}</option></select> · {{ entry.metadata?.lineEnding }}{{ entry.metadata?.bom ? ' · BOM' : '' }}{{ entry.busy ? ` · ${t('editor.busy')}` : '' }}</div>
    <NModal :show="!!entry.dialog" preset="card" :title="t('editor.dialogTitle')" class="form-modal" @update:show="(show) => { if (!show) answer('cancel') }">
      <p>{{ entry.dialog?.message }}</p><div class="dialog-actions"><NButton @click="answer('cancel')">{{ t('common.cancel') }}</NButton><template v-if="entry.dialog?.kind === 'conflict'"><NButton @click="answer('reload')">{{ t('editor.reload') }}</NButton><NButton type="warning" @click="answer('overwrite')">{{ t('editor.overwrite') }}</NButton></template><NButton v-else-if="entry.dialog?.kind === 'unmappable'" @click="answer('utf8')">{{ t('editor.saveUtf8') }}</NButton><NButton v-else @click="answer('reload')">{{ t('editor.discardReopen') }}</NButton></div>
    </NModal>
  </div>
</template>
