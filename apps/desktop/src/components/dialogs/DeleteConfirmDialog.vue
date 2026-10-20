<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { NAlert, NButton, NModal } from 'naive-ui'
import { projectApi } from '../../services/projectApi'
import { useProjects } from '../../stores/projects'
import { t } from '../../i18n'
const store = useProjects()
const props = defineProps<{ confirmProjectChange: (id: string) => Promise<boolean> }>()
const deleting = defineModel<{ kind: 'project' | 'group'; id: string; name: string } | null>('item', { required: true })
async function confirmDelete() {
  const item = deleting.value
  if (item?.kind === 'project' && !await props.confirmProjectChange(item.id)) return
  if (
    item &&
    (await store.run(() =>
      item.kind === 'project'
        ? projectApi.deleteProject(item.id)
        : projectApi.deleteGroup(item.id),
    ))
  )
    deleting.value = null
}
</script>
<template>
      <NModal
        :show="!!deleting"
        preset="card"
        :title="t('delete.title')"
        class="form-modal"
        @update:show="
          (value) => {
            if (!value) deleting = null
          }
        "
        ><p>{{ t('delete.question', { name: deleting?.name ?? '' }) }}</p>
        <p>
          {{
            deleting?.kind === 'project'
              ? t('delete.project')
              : t('delete.group')
          }}
        </p>
        <NAlert v-if="store.error" type="error">{{ store.error }}</NAlert>
        <div class="dialog-actions">
          <NButton @click="deleting = null">{{ t('common.cancel') }}</NButton
          ><NButton type="error" :loading="store.busy" @click="confirmDelete"
            >{{ t('common.delete') }}</NButton
          >
        </div></NModal
      >
</template>
