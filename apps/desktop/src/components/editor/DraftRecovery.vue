<!-- Author: jaswtao · Date: 2026-10-24 -->
<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { NButton, NModal } from 'naive-ui'
import { fileApi, type EditorDraft } from '../../services/fileApi'
import type { Project } from '../../services/projectApi'
import type { EditorRegistry } from '../../services/editor/registry'
import { invoke } from '../../services/diagnostics'
import { t } from '../../i18n'
const props = defineProps<{ projects: Project[]; editors: EditorRegistry }>()
const emit = defineEmits<{ open: [draft: EditorDraft]; visibility: [show: boolean]; error: [message: string] }>()
const drafts = ref<EditorDraft[]>([]), show = ref(false), selected = ref<EditorDraft | null>(null), busy = ref(false)
watch(show, (visible) => emit('visibility', visible), { flush: 'sync' })
function rootChanged(draft: EditorDraft) { return props.projects.find((p) => p.id === draft.projectId)?.path.toLowerCase() !== draft.projectPath.toLowerCase() }
async function refresh() { try { drafts.value = await fileApi.drafts() } catch { emit('error', t('draft.failed')) } }
async function discard(draft: EditorDraft) {
  busy.value = true
  try {
    await props.editors.deleteDraft(draft)
    selected.value = null; await refresh()
  } catch { emit('error', t('draft.failed')) }
  finally { busy.value = false }
}
async function copy(draft: EditorDraft) { try { await invoke('write_clipboard_text', { text: draft.content }) } catch { emit('error', t('draft.failed')) } }
onMounted(async () => { await refresh(); show.value = drafts.value.length > 0 })
</script>
<template>
  <button @click="refresh(); show = true">{{ t('draft.title') }}</button>
  <NModal v-model:show="show" preset="card" :title="t('draft.title')" class="form-modal">
    <p>{{ t('draft.hint') }}</p>
    <p v-if="!drafts.length">{{ t('draft.empty') }}</p>
    <div class="draft-recovery-list">
    <div v-for="draft in drafts" :key="`${draft.projectId}:${draft.path}`">
      <p>{{ projects.find((p) => p.id === draft.projectId)?.name }} · {{ draft.path }} · {{ draft.updatedAt }}</p>
      <p v-if="rootChanged(draft)">{{ t('draft.rootChanged') }}</p>
      <div class="dialog-actions">
        <NButton :disabled="busy || rootChanged(draft)" @click="show = false; emit('open', draft)">{{ t('draft.open') }}</NButton>
        <NButton @click="selected = draft">{{ t('draft.view') }}</NButton>
        <NButton @click="copy(draft)">{{ t('draft.copy') }}</NButton>
        <NButton :disabled="busy" type="error" @click="discard(draft)">{{ t('draft.discard') }}</NButton>
      </div>
    </div>
    </div>
    <textarea v-if="selected" :value="selected.content" readonly :aria-label="t('draft.view')" class="draft-preview" />
  </NModal>
</template>
