<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { ref, watch } from 'vue'
import { NAlert, NButton, NForm, NFormItem, NInput, NModal, NSwitch } from 'naive-ui'
import { gitApi, type Worktree } from '../../services/gitApi'
import { projectApi, type Project } from '../../services/projectApi'
import { useProjects } from '../../stores/projects'
import { useGit } from '../../stores/git'
import { t } from '../../i18n'
const props = defineProps<{ project: Project | null }>()
const show = defineModel<boolean>('show', { required: true })
const store = useProjects()
const git = useGit()
const trees = ref<Worktree[]>([])
const branch = ref('')
const path = ref('')
const createBranch = ref(true)
const busy = ref(false)
const error = ref('')
let pathEdited = false
function suggestedPath() { return `${props.project?.path ?? ''}-${branch.value.replace(/[<>:"/\\|?*\x00-\x1f]/g, '-')}` }
watch(branch, () => { if (!pathEdited) path.value = suggestedPath() })
watch(show, async (visible) => {
  if (!visible || !props.project) return
  branch.value = ''; pathEdited = false; path.value = suggestedPath(); error.value = ''; trees.value = []
  const id = props.project.id
  try { const result = await gitApi.worktrees(id); if (show.value && props.project?.id === id) trees.value = result }
  catch (reason) { error.value = String(reason) }
})
async function browse() {
  try { const directory = await projectApi.chooseDirectory(); if (directory) { path.value = `${directory.replace(/[\\/]+$/, '')}\\${props.project?.path.split(/[\\/]/).pop()}-${branch.value.replace(/[<>:"/\\|?*]/g, '-')}`; pathEdited = true } }
  catch (reason) { error.value = String(reason) }
}
async function add(tree?: Worktree) {
  if (!props.project || busy.value) return
  busy.value = true; error.value = ''
  const project = props.project
  try {
    const succeeded = await store.run(async () => {
      const id = tree ? await projectApi.createProject({ name: `${project.name} · ${tree.branch ?? 'detached'}`, path: tree.path, groupId: project.groupId, favorite: false }) : await gitApi.addWorktree(project.id, path.value, branch.value, createBranch.value)
      await projectApi.setSetting('active_project_id', id)
    })
    if (succeeded) { git.schedule(project.id); show.value = false } else error.value = store.error
  } finally { busy.value = false }
}
</script>
<template>
  <NModal :show="show" preset="card" title="Git Worktree" class="form-modal" :mask-closable="!busy" :closable="!busy" @update:show="(value) => { if (!busy) show = value }">
    <p v-for="tree in trees" :key="tree.path">{{ tree.branch ?? 'detached' }} · {{ tree.path }} <NButton size="small" :disabled="busy || !!tree.projectId" @click="add(tree)">{{ tree.projectId ? t('worktree.added') : t('worktree.add') }}</NButton></p>
    <NForm @submit.prevent="add()">
      <NFormItem :label="t('worktree.branch')"><NInput v-model:value="branch" :disabled="busy" /></NFormItem>
      <NFormItem :label="t('worktree.newBranch')"><NSwitch v-model:value="createBranch" :disabled="busy" /></NFormItem>
      <NFormItem :label="t('worktree.directory')"><div class="path-picker"><NInput v-model:value="path" :disabled="busy" @update:value="pathEdited = true" /><NButton :disabled="busy" @click="browse">{{ t('worktree.browseParent') }}</NButton></div></NFormItem>
      <NAlert v-if="error" type="error">{{ error }}</NAlert>
      <div class="dialog-actions"><NButton :disabled="busy" @click="show = false">{{ t('common.cancel') }}</NButton><NButton attr-type="submit" :loading="busy" :disabled="!branch.trim() || !path.trim() || store.busy">{{ t('worktree.create') }}</NButton></div>
    </NForm>
  </NModal>
</template>
