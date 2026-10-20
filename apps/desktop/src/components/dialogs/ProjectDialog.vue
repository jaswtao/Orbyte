<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { NAlert, NButton, NForm, NFormItem, NInput, NModal, NSelect, NSwitch } from 'naive-ui'
import { projectApi, type Project, type ProjectInput } from '../../services/projectApi'
import { useProjects } from '../../stores/projects'
import { t } from '../../i18n'
const store = useProjects()
const props = defineProps<{ project: Project | null; defaultGroupId?: string; confirmProjectChange: (id: string) => Promise<boolean> }>()
const show = defineModel<boolean>('show', { required: true })
const editingId = computed(() => props.project?.id ?? null)
const form = reactive({ name: '', path: '', groupId: '', favorite: false })
const groupOptions = computed(() => [{ label: t('sidebar.ungrouped'), value: '' }, ...store.groups.map((g) => ({ label: g.name, value: g.id }))])
watch(show, (visible) => { if (visible) Object.assign(form, props.project ? { ...props.project, groupId: props.project.groupId ?? '' } : { name: '', path: '', groupId: props.defaultGroupId ?? '', favorite: false }) })
async function chooseDirectory() {
  try {
    const path = await projectApi.chooseDirectory(
      form.path || store.settings?.default_directory || undefined,
    )
    if (path) {
      form.path = path
      if (!form.name)
        form.name =
          path
            .replace(/[\\/]+$/, '')
            .split(/[\\/]/)
            .pop() || path
    }
  } catch (e) {
    store.error = String(e)
  }
}
async function saveProject() {
  const input: ProjectInput = { ...form, groupId: form.groupId || null }
  const previous = store.projects.find((p) => p.id === editingId.value)
  if (previous && previous.path.toLowerCase() !== input.path.toLowerCase() && !await props.confirmProjectChange(previous.id)) return
  if (
    await store.run(async () => {
      if (editingId.value)
        await projectApi.updateProject(editingId.value, input)
      else {
        const id = await projectApi.createProject(input)
        await projectApi.setSetting('active_project_id', id)
      }
    })
  )
    show.value = false
}
</script>
<template>
      <NModal
        v-model:show="show"
        preset="card"
        :title="editingId ? t('project.edit') : t('project.add')"
        class="form-modal"
      >
        <NForm @submit.prevent="saveProject"
          ><NFormItem :label="t('project.name')"
            ><NInput v-model:value="form.name" :maxlength="100" /></NFormItem
          ><NFormItem :label="t('project.directory')"
            ><div class="path-picker">
              <NInput
                v-model:value="form.path"
                placeholder="C:\projects\my-project"
              /><NButton @click="chooseDirectory">{{ t('common.browse') }}</NButton>
            </div></NFormItem
          ><NFormItem :label="t('project.group')"
            ><NSelect
              v-model:value="form.groupId"
              :options="groupOptions" /></NFormItem
          ><NFormItem :label="t('project.favorite')"
            ><NSwitch v-model:value="form.favorite" /></NFormItem
          ><NAlert v-if="store.error" type="error">{{ store.error }}</NAlert>
          <div class="dialog-actions">
            <NButton @click="show = false">{{ t('common.cancel') }}</NButton
            ><NButton
              attr-type="submit"
              type="primary"
              :loading="store.busy"
              :disabled="!form.name.trim() || !form.path.trim()"
              >{{ t('common.save') }}</NButton
            >
          </div></NForm
        >
      </NModal>
</template>
