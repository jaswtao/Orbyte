<!-- Author: jaswtao · Date: 2026-10-16 -->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NAlert, NButton, NModal } from 'naive-ui'
import ShortcutSettings from './ShortcutSettings.vue'
import { projectApi } from '../services/projectApi'
import { shortcutConflicts, type ShortcutOverrides } from '../services/workspace/shortcuts'
import { useProjects } from '../stores/projects'
import { t } from '../i18n'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [show: boolean] }>()
const store = useProjects()
const draft = ref<ShortcutOverrides>({})
const saving = ref(false)
const error = ref('')
const conflicts = computed(() => shortcutConflicts(draft.value))
watch(() => props.show, (show) => {
  if (!show) return
  draft.value = JSON.parse(JSON.stringify(store.settings?.shortcuts ?? {}))
  error.value = ''
})
async function save() {
  if (saving.value || store.busy || conflicts.value.length) return
  saving.value = true
  error.value = ''
  try {
    if (!await store.run(() => projectApi.setSetting('shortcuts', draft.value))) {
      error.value = store.error
      return
    }
    emit('update:show', false)
  } finally { saving.value = false }
}
</script>

<template>
  <NModal :show="show" preset="card" :title="t('settings.shortcutSettings')" class="form-modal settings-modal shortcut-settings-modal" :mask-closable="!saving" :closable="!saving" :close-on-esc="!saving" @update:show="(value) => { if (!saving) emit('update:show', value) }">
    <div class="settings-form">
      <div class="settings-scroll">
        <ShortcutSettings v-model="draft" :active="show" :disabled="saving" />
        <NAlert v-if="error" type="error">{{ error }}</NAlert>
      </div>
      <div class="dialog-actions">
        <NButton :disabled="saving" @click="emit('update:show', false)">{{ t('common.cancel') }}</NButton>
        <NButton type="primary" :loading="saving" :disabled="!!conflicts.length || store.busy" @click="save">{{ t('common.save') }}</NButton>
      </div>
    </div>
  </NModal>
</template>

<style scoped>
.shortcut-settings-modal { width: 800px; }
</style>
