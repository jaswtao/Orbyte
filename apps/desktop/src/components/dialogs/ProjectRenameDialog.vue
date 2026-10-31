<!-- Author: jaswtao · Date: 2026-10-30 -->
<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { NAlert, NButton, NInput, NModal } from 'naive-ui'
import { projectApi, type Project } from '../../services/projectApi'
import { useProjects } from '../../stores/projects'
import { t } from '../../i18n'

const project = defineModel<Project | null>('project', { required: true })
const store = useProjects()
const name = ref('')
const error = ref('')
const saving = ref(false)
const input = ref<{ focus: () => void; select: () => void } | null>(null)
watch(project, async (value) => {
  if (!value) return
  name.value = value.name
  error.value = ''
  await nextTick()
  input.value?.focus()
  input.value?.select()
})
function cancel() { if (!saving.value) project.value = null }
async function save() {
  const current = store.projects.find((item) => item.id === project.value?.id)
  const title = name.value.trim()
  if (!current || !title || title.length > 100 || saving.value || store.busy || !store.ready) return
  saving.value = true
  error.value = ''
  try {
    if (await store.run(() => projectApi.updateProject(current.id, { ...current, name: title }))) project.value = null
    else error.value = store.error || t('project.renameFailed')
  } finally { saving.value = false }
}
</script>

<template>
  <NModal :show="!!project" preset="card" :title="t('project.rename')" class="form-modal" :mask-closable="!saving" :close-on-esc="!saving" @update:show="(show) => { if (!show) cancel() }">
    <form @submit.prevent="save">
      <NInput v-if="project" ref="input" v-model:value="name" :maxlength="100" :disabled="saving" :aria-label="t('project.name')" />
      <NAlert v-if="error" type="error">{{ error }}</NAlert>
      <div class="dialog-actions">
        <NButton :disabled="saving" @click="cancel">{{ t('common.cancel') }}</NButton>
        <NButton attr-type="submit" :loading="saving" :disabled="store.busy || !store.ready || !name.trim()">{{ t('common.save') }}</NButton>
      </div>
    </form>
  </NModal>
</template>
