<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { computed, nextTick, watch } from 'vue'
import RemoteWorkspace from './RemoteWorkspace.vue'
import { useRemote } from '../../stores/remote'
import type { Settings } from '../../services/projectApi'
import type { AppearanceMode } from '../../services/appearance'
import { t } from '../../i18n'

const props = defineProps<{ active: boolean; settings: Settings; appearance: AppearanceMode; shortcutsBlocked: boolean }>()
const emit = defineEmits<{ error: [message: string] }>()
const remote = useRemote()
const views = new Map<string, InstanceType<typeof RemoteWorkspace>>()
const retained = computed(() => remote.projects.filter((project) => remote.visitedProjects.has(project.id)))
function focusTerminal() { if (remote.activeProjectId) views.get(remote.activeProjectId)?.focusTerminal() }
function renameActiveTab() { return !!remote.activeProjectId && (views.get(remote.activeProjectId)?.renameActiveTab() ?? false) }
function modalVisible() {
  return Array.from(document.querySelectorAll('[aria-modal="true"]')).some((element) => element.getClientRects().length > 0)
}
function cycleProject(delta: number): string | undefined {
  if (!props.active || props.shortcutsBlocked || modalVisible()) return
  const projects = remote.projects
  if (!projects.length) return
  const index = projects.findIndex((project) => project.id === remote.activeProjectId)
  const project = projects[index < 0 ? (delta < 0 ? projects.length - 1 : 0) : (index + delta + projects.length) % projects.length]
  if (!project) return
  remote.selectProject(project.id)
  void nextTick(() => { if (props.active && !props.shortcutsBlocked && remote.activeProjectId === project.id && !modalVisible()) focusTerminal() })
  return project.id
}
watch(() => [props.active, remote.activeProjectId], () => {
  if (props.active && remote.activeProjectId) remote.visitedProjects.add(remote.activeProjectId)
}, { immediate: true })
defineExpose({ focusTerminal, renameActiveTab, cycleProject })
</script>
<template>
  <div v-show="active" class="remote-deck">
    <div v-if="!remote.activeProjectId" class="workspace-placeholder">
      <div class="welcome-copy"><h1>{{ t('remote.projects') }}</h1><p>{{ t('remote.noProjects') }}</p></div>
    </div>
    <RemoteWorkspace v-for="project in retained" v-show="active && project.id === remote.activeProjectId" :key="project.id" :ref="(view) => { if (view) views.set(project.id, view as InstanceType<typeof RemoteWorkspace>); else views.delete(project.id) }" :workspace-id="project.id" :settings="settings" :appearance="appearance" :active="active && project.id === remote.activeProjectId" :shortcuts-blocked="shortcutsBlocked" @error="emit('error', $event)" />
  </div>
</template>
