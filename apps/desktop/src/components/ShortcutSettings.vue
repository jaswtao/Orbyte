<!-- Author: jaswtao · Date: 2026-10-16 -->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { NAlert, NButton } from 'naive-ui'
import { bindings, commandLabel, formatShortcut, shortcutCommands, shortcutConflicts, shortcutGroupLabel, shortcutHint, shortcutScopes, validShortcut, type ShortcutKey, type ShortcutOverrides } from '../services/workspace/shortcuts'
import { t } from '../i18n'

const props = defineProps<{ modelValue: ShortcutOverrides; active: boolean; disabled: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: ShortcutOverrides] }>()
const recording = ref<{ id: string; index: number } | null>(null)
const message = ref('')
const conflicts = computed(() => shortcutConflicts(props.modelValue))
function update(id: string, keys: ShortcutKey[]) {
  emit('update:modelValue', { ...props.modelValue, [id]: keys })
}
function remove(id: string, index: number) {
  recording.value = null
  update(id, bindings(id, props.modelValue).filter((_, i) => i !== index))
}
function restore(id?: string) {
  recording.value = null
  message.value = ''
  const next = { ...props.modelValue }
  if (id) delete next[id]
  emit('update:modelValue', id ? next : {})
}
function capture(event: KeyboardEvent) {
  if (!recording.value || !props.active || props.disabled) return
  event.preventDefault()
  event.stopImmediatePropagation()
  if (event.isComposing || event.repeat) return
  if (event.code === 'Escape') { recording.value = null; message.value = ''; return }
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return
  const key: ShortcutKey = { code: event.code, ctrlKey: event.ctrlKey, altKey: event.altKey, shiftKey: event.shiftKey, metaKey: event.metaKey }
  if (!validShortcut(key)) { message.value = t('shortcut.pressModifier'); return }
  const { id, index } = recording.value
  const keys = [...bindings(id, props.modelValue)]
  keys[index] = key
  update(id, keys)
  recording.value = null
  message.value = ''
}
watch(() => [props.active, props.disabled], () => { recording.value = null; message.value = '' })
onMounted(() => window.addEventListener('keydown', capture, true))
onBeforeUnmount(() => window.removeEventListener('keydown', capture, true))
</script>

<template>
  <fieldset class="settings-section shortcut-settings">
    <legend>{{ t('settings.shortcuts') }}</legend>
    <div class="shortcut-toolbar"><span>{{ t('shortcut.recordHint') }}</span><NButton size="small" :disabled="disabled" @click="restore()">{{ t('shortcut.restoreAll') }}</NButton></div>
    <NAlert v-if="recording" type="info">{{ t('shortcut.recording') }}<NButton size="tiny" @click="recording = null">{{ t('shortcut.cancelRecording') }}</NButton></NAlert>
    <NAlert v-if="message" type="warning">{{ message }}</NAlert>
    <NAlert v-if="conflicts.length" type="error"><div v-for="conflict in conflicts" :key="conflict">{{ conflict }}</div></NAlert>
    <section v-for="scope in shortcutScopes" :key="scope">
      <h4>{{ shortcutGroupLabel(scope) }}</h4>
      <div v-for="command in shortcutCommands.filter((item) => item.scope === scope)" :key="command.id" class="shortcut-row">
        <div class="shortcut-label">{{ commandLabel(command.id) }}<small>{{ t('shortcut.defaultHint', { hint: shortcutHint(command.id) }) }}</small></div>
        <div class="shortcut-bindings">
          <span v-for="(key, index) in bindings(command.id, modelValue)" :key="index" class="shortcut-binding">
            <NButton size="small" :disabled="disabled" :type="recording?.id === command.id && recording.index === index ? 'primary' : 'default'" @click="recording = { id: command.id, index }; message = ''">{{ recording?.id === command.id && recording.index === index ? t('shortcut.pressCombo') : formatShortcut(key) }}</NButton>
            <NButton size="tiny" :disabled="disabled" :aria-label="t('shortcut.removeBinding', { label: commandLabel(command.id), shortcut: formatShortcut(key) })" @click="remove(command.id, index)">×</NButton>
          </span>
          <small v-if="!bindings(command.id, modelValue).length">{{ t('shortcut.unbound') }}</small>
          <NButton size="tiny" :disabled="disabled" @click="recording = { id: command.id, index: bindings(command.id, modelValue).length }; message = ''">{{ t('shortcut.add') }}</NButton>
          <NButton size="tiny" :disabled="disabled" @click="recording = null; update(command.id, [])">{{ t('shortcut.clear') }}</NButton>
          <NButton size="tiny" :disabled="disabled" @click="restore(command.id)">{{ t('shortcut.restoreDefault') }}</NButton>
        </div>
      </div>
    </section>
  </fieldset>
</template>

<style scoped>
.shortcut-toolbar { display: flex; gap: 12px; align-items: center; justify-content: space-between; margin-bottom: 12px; flex-wrap: wrap; }
.shortcut-row { display: flex; flex-wrap: wrap; gap: 8px; padding: 10px 0; border-bottom: 1px solid var(--border); }
.shortcut-label { flex: 1 1 180px; }
.shortcut-label small { display: block; opacity: .65; font-size: 11px; }
.shortcut-bindings { display: flex; flex: 1 1 280px; flex-wrap: wrap; gap: 6px; align-items: center; }
.shortcut-binding { display: inline-flex; align-items: center; gap: 2px; }
.shortcut-settings :deep(.n-alert) { margin-bottom: 10px; }
</style>
