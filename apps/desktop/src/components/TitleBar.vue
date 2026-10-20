<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import UiIcon from './UiIcon.vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { t } from '../i18n'

defineProps<{ sidebarVisible: boolean; settingsDisabled: boolean }>()
const emit = defineEmits<{ toggleSidebar: []; settings: []; error: [message: string] }>()
const maximized = ref(false)
let unlistenResize: (() => void) | undefined
let disposed = false

async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
  try {
    const current = getCurrentWindow()
    await current[action]()
    if (action === 'toggleMaximize') maximized.value = await current.isMaximized()
  } catch (reason) { emit('error', t('titlebar.windowActionFailed', { reason: String(reason) })) }
}

onMounted(async () => {
  try {
    const current = getCurrentWindow()
    unlistenResize = await current.onResized(async () => {
      try { maximized.value = await current.isMaximized() }
      catch (reason) { if (!disposed) emit('error', t('titlebar.windowStateFailed', { reason: String(reason) })) }
    })
    if (disposed) { unlistenResize(); return }
    maximized.value = await current.isMaximized()
  } catch (reason) { emit('error', t('titlebar.windowInitFailed', { reason: String(reason) })) }
})
onBeforeUnmount(() => { disposed = true; unlistenResize?.() })
</script>

<template>
  <header class="titlebar">
    <div class="titlebar-sidebar" :class="{ collapsed: !sidebarVisible }" data-tauri-drag-region>
      <button class="titlebar-button sidebar-toggle" :aria-label="sidebarVisible ? t('titlebar.collapseSidebar') : t('titlebar.expandSidebar')" :title="sidebarVisible ? t('titlebar.collapseSidebar') : t('titlebar.expandSidebar')" :aria-expanded="sidebarVisible" aria-controls="workbench-sidebar" @click="emit('toggleSidebar')">
        <svg width="18" height="18" viewBox="0 0 20 20" fill="none" aria-hidden="true"><rect x="2.5" y="3.5" width="15" height="13" rx="1.5" stroke="currentColor"/><path d="M7 4v12" stroke="currentColor"/></svg>
      </button>
      <span v-if="sidebarVisible" class="app-wordmark" data-tauri-drag-region>Orbyte</span>
    </div>
    <div class="titlebar-tabs" data-tauri-drag-region><slot /></div>
    <div class="titlebar-drag-space" data-tauri-drag-region />
    <button class="titlebar-button settings-button" :disabled="settingsDisabled" :aria-label="t('titlebar.settings')" :title="t('titlebar.settings')" @click="emit('settings')"><UiIcon name="settings" /></button>
    <div class="window-controls">
      <button class="titlebar-button" :aria-label="t('titlebar.minimize')" :title="t('titlebar.minimize')" @click="windowAction('minimize')"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1 6h10" fill="none" stroke="currentColor"/></svg></button>
      <button class="titlebar-button" :aria-label="maximized ? t('titlebar.restore') : t('titlebar.maximize')" :title="maximized ? t('titlebar.restore') : t('titlebar.maximize')" @click="windowAction('toggleMaximize')"><svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" aria-hidden="true"><path v-if="maximized" d="M3.5 3.5v-2h7v7h-2m-7-5h7v7h-7z"/><rect v-else x="1.5" y="1.5" width="9" height="9"/></svg></button>
      <button class="titlebar-button window-close" :aria-label="t('titlebar.close')" :title="t('titlebar.close')" @click="windowAction('close')"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="m1.5 1.5 9 9m0-9-9 9" fill="none" stroke="currentColor"/></svg></button>
    </div>
  </header>
</template>
