<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NAlert, NButton, NForm, NFormItem, NInput, NModal } from 'naive-ui'
import { projectApi, type ProjectGroup } from '../../services/projectApi'
import { useProjects } from '../../stores/projects'
import { t } from '../../i18n'
const store = useProjects()
const props = defineProps<{ group: ProjectGroup | null }>()
const show = defineModel<boolean>('show', { required: true })
const groupId = computed(() => props.group?.id ?? null)
const groupName = ref('')
watch(show, (visible) => { if (visible) groupName.value = props.group?.name ?? '' })
async function saveGroup() {
  if (
    await store.run(() =>
      groupId.value
        ? projectApi.updateGroup(groupId.value, groupName.value)
        : projectApi.createGroup(groupName.value),
    )
  )
    show.value = false
}
</script>
<template>
      <NModal
        v-model:show="show"
        preset="card"
        :title="groupId ? t('group.rename') : t('group.create')"
        class="form-modal"
        ><NForm @submit.prevent="saveGroup"
          ><NFormItem :label="t('group.name')"
            ><NInput v-model:value="groupName" :maxlength="100" /></NFormItem
          ><NAlert v-if="store.error" type="error">{{ store.error }}</NAlert>
          <div class="dialog-actions">
            <NButton @click="show = false">{{ t('common.cancel') }}</NButton
            ><NButton
              attr-type="submit"
              type="primary"
              :loading="store.busy"
              :disabled="!groupName.trim()"
              >{{ t('common.save') }}</NButton
            >
          </div></NForm
        ></NModal
      >
</template>
