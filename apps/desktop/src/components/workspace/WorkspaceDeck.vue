<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowReactive, watch } from 'vue'
import { NButton, NModal } from 'naive-ui'
import type { Project, Settings } from '../../services/projectApi'
import { terminalApi } from '../../services/terminalApi'
import type { AppearanceMode } from '../../services/appearance'
import { TerminalRegistry } from '../../services/terminal/registry'
import WorkspaceView from './WorkspaceView.vue'
import { EditorRegistry } from '../../services/editor/registry'
import type { FileChange } from '../../services/fileApi'
import '@xterm/xterm/css/xterm.css'
import './workspace.css'
import { t } from '../../i18n'

const props = defineProps<{ tabsTarget: string; appearance: AppearanceMode; projects: Project[]; settings: Settings; activeId: string | null; shortcutsBlocked: boolean }>()
const emit = defineEmits<{ error: [message: string]; filesChanged: [projectId: string, change: FileChange]; activity: [projectId: string, state: import('../../services/workspace/activity').Activity] }>()
const visited = reactive(new Set<string>())
const registry = new TerminalRegistry(() => props.appearance, () => props.settings, () => props.shortcutsBlocked || !!discard.value)
const sessionTimer = setInterval(() => { void registry.syncSessions() }, 2000)
onBeforeUnmount(() => clearInterval(sessionTimer))
const editors = new EditorRegistry((message) => emit('error', message), () => props.settings, () => props.shortcutsBlocked || !!discard.value)
const views = shallowReactive(new Map<string, InstanceType<typeof WorkspaceView>>())
const mutationBusy = computed(() => !props.activeId || !views.has(props.activeId) || views.get(props.activeId)?.busy || Array.from(editors.entries.values()).some((entry) => entry.projectId === props.activeId && entry.busy))
const discard = ref<{ projectId: string; resolve: (proceed: boolean) => void } | null>(null)
function openFile(path: string) { if (props.activeId) void views.get(props.activeId)?.openFile(path) }
function renamed(from: string, to: string) { if (props.activeId) void views.get(props.activeId)?.renamed(from, to) }
function confirmProjectChange(projectId: string) {
  if (!Array.from(editors.entries.values()).some((e) => e.projectId === projectId && e.document.dirty)) return Promise.resolve(true)
  return new Promise<boolean>((resolve) => { discard.value = { projectId, resolve } })
}
const discardSaving = ref(false)
async function resolveDiscard(save: boolean, proceed: boolean) {
  const pending = discard.value
  if (!pending || discardSaving.value) return
  if (save) {
    discardSaving.value = true
    try {
      for (const entry of editors.entries.values()) if (entry.projectId === pending.projectId && entry.document.dirty) {
        await editors.save(entry.id)
        if (entry.document.dirty) { emit('error', t('workspace.stillUnsaved')); return }
      }
    } finally { discardSaving.value = false }
  }
  discard.value = null; pending.resolve(proceed)
}
async function saveAllEditors(): Promise<boolean> {
  for (const entry of editors.entries.values()) {
    if (!entry.document.dirty) continue
    await editors.save(entry.id)
    if (entry.document.dirty) return false
  }
  // 保存期间仍可能有输入；只有全部文档保持干净才可退出。
  return !Array.from(editors.entries.values()).some((entry) => entry.document.dirty)
}
watch([
  () => props.settings.shortcuts,
  () => props.settings.font_family,
  () => props.settings.font_size,
  () => props.settings.scrollback,
  () => props.settings.theme,
  () => props.appearance,
], () => {
  editors.updateSettings(props.settings, props.appearance)
  registry.updateSettings(props.settings)
}, { flush: 'post' })
defineExpose({ openFile, renamed, confirmProjectChange, mutationBusy, saveAllEditors })
const retained = computed(() => props.projects.filter((project) => visited.has(project.id)))
watch(() => props.activeId, (id) => { if (id) visited.add(id) }, { immediate: true })
onMounted(async () => {
  try {
    // 重载时为已有运行时会话恢复订阅；无会话的非活动项目仍保持懒启动。
    const running = new Set(await terminalApi.listRunningWorkspaces())
    for (const project of props.projects) if (running.has(project.workspaceId)) visited.add(project.id)
  } catch (reason) { emit('error', String(reason)) }
})
</script>
<template>
  <div class="workspace-deck">
    <WorkspaceView v-for="project in retained" :key="`${project.id}:${project.path}`" :ref="(view) => { if (view) views.set(project.id, view as InstanceType<typeof WorkspaceView>); else views.delete(project.id) }" :tabs-target="tabsTarget" :project="project" :settings="settings" :active="activeId === project.id" :registry="registry" :editors="editors" :shortcuts-blocked="shortcutsBlocked || !!discard" @activity="(id, state) => emit('activity', id, state)" @error="emit('error', $event)" @files-changed="(id, change) => emit('filesChanged', id, change)" />
    <NModal :show="!!discard" preset="card" :title="t('workspace.unsavedTitle')" class="form-modal" @update:show="(show) => { if (!show) resolveDiscard(false, false) }"><p>{{ t('workspace.unsavedBody') }}</p><div class="dialog-actions"><NButton :disabled="discardSaving" @click="resolveDiscard(false, false)">{{ t('common.cancel') }}</NButton><NButton :loading="discardSaving" @click="resolveDiscard(true, true)">{{ t('workspace.saveAllContinue') }}</NButton><NButton type="error" :disabled="discardSaving" @click="resolveDiscard(false, true)">{{ t('workspace.discardContinue') }}</NButton></div></NModal>
  </div>
</template>
