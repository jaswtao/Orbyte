<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import ShortcutSettingsDialog from './ShortcutSettingsDialog.vue'
import ReleaseNotesDialog from './ReleaseNotesDialog.vue'
import UiIcon from './UiIcon.vue'
import { computed, reactive, ref, watch } from 'vue'
import { NAlert, NButton, NColorPicker, NForm, NFormItem, NInput, NInputNumber, NModal, NSelect, NSlider, NSwitch, NTooltip, useOsTheme } from 'naive-ui'
import { appearanceVariables, palettes, resolveAppearance } from '../services/appearance'
import { releaseSummary } from '../services/release'
import { projectApi, type Settings } from '../services/projectApi'
import { terminalApi, type ShellInfo } from '../services/terminalApi'
import { useProjects } from '../stores/projects'
import { shellLabel, t } from '../i18n'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [show: boolean] }>()
const store = useProjects()
const draft = reactive<Settings>({ shortcuts: {}, active_project_id: null, show_workspace_header: false, show_hidden_projects: false, shell_integration: true, confirm_close_terminal: true, notify_command_seconds: 30, theme: 'dark', workspace_border_color: null, workspace_border_intensity: 100, font_family: '', font_size: 14, scrollback: 5000, default_shell: 'powershell', default_directory: null, language: 'en' })
const osTheme = useOsTheme()
const draftAppearance = computed(() => resolveAppearance(draft.theme, osTheme.value))
const borderColor = computed({
  get: () => draft.workspace_border_color ?? palettes[draftAppearance.value].accent,
  set: (value: string) => { draft.workspace_border_color = value },
})
const borderPreview = computed(() => appearanceVariables(draftAppearance.value, draft.workspace_border_color, draft.workspace_border_intensity))
function resetWorkspaceBorder() {
  draft.workspace_border_color = null
  draft.workspace_border_intensity = 100
}
const error = ref('')
const saving = ref(false)
const shortcutsDialog = ref(false)
const releaseDialog = ref(false)
const childDialogOpen = computed(() => shortcutsDialog.value || releaseDialog.value)
const shells = ref<ShellInfo[]>([])
const shellHelpHovered = ref(false)
const shellHelpFocused = ref(false)
const shellOptions = computed(() => shells.value.map((shell) => ({ label: shellLabel(shell), value: shell.shellType, disabled: !shell.available })))
const themeOptions = computed(() => [
  { label: t('settings.themeSystem'), value: 'system' },
  { label: t('settings.themeDark'), value: 'dark' },
  { label: t('settings.themeLight'), value: 'light' },
])
watch(() => props.show, async (show) => {
  if (!show) releaseDialog.value = false
  if (!show || !store.settings) return
  Object.assign(draft, store.settings)
  error.value = ''
  try { shells.value = await terminalApi.listAvailableShells() }
  catch (reason) { error.value = String(reason) }
})
async function chooseDirectory() {
  try {
    const path = await projectApi.chooseDirectory(draft.default_directory ?? undefined)
    if (path) draft.default_directory = path
  } catch (reason) { error.value = String(reason) }
}
async function save() {
  if (saving.value || store.busy || childDialogOpen.value || !store.settings) return
  saving.value = true
  error.value = ''
  try {
    const { theme, workspace_border_color, workspace_border_intensity, font_family, font_size, scrollback, default_shell, shell_integration, confirm_close_terminal, notify_command_seconds, language } = draft
    const shortcuts = store.settings.shortcuts
    if (!await store.run(() => projectApi.setSettings({ shortcuts, theme, workspace_border_color, workspace_border_intensity, font_family, font_size, scrollback, default_shell, shell_integration, confirm_close_terminal, notify_command_seconds, language, default_directory: draft.default_directory?.trim() || null }))) {
      error.value = store.error
      return
    }
    emit('update:show', false)
  } finally { saving.value = false }
}
</script>
<template>
  <NModal :show="show" preset="card" :title="t('settings.title')" class="form-modal settings-modal" :mask-closable="!saving && !childDialogOpen" :closable="!saving && !childDialogOpen" :close-on-esc="!saving && !childDialogOpen" @update:show="(value) => { if (!saving && !childDialogOpen) emit('update:show', value) }">
    <NForm class="settings-form" label-placement="left" :label-width="190" label-align="left" :show-feedback="false" :disabled="saving || childDialogOpen" @submit.prevent="save">
      <div class="settings-scroll"><fieldset class="settings-section"><legend>{{ t('settings.language') }}</legend>
      <NFormItem :label="t('settings.language')"><NSelect v-model:value="draft.language" :options="[{ label: 'English', value: 'en' }, { label: '中文', value: 'zh' }]" /></NFormItem>
      </fieldset><fieldset class="settings-section"><legend>{{ t('settings.appearance') }}</legend>
      <NFormItem :label="t('settings.theme')"><NSelect v-model:value="draft.theme" :options="themeOptions" /></NFormItem>
      <NFormItem :label="t('settings.workspaceBorderColor')"><NColorPicker v-model:value="borderColor" :modes="['hex']" :show-alpha="false" /></NFormItem>
      <NFormItem :label="t('settings.workspaceBorderIntensity')"><div class="border-intensity"><NSlider v-model:value="draft.workspace_border_intensity" :min="0" :max="100" :step="1" :aria-label="t('settings.workspaceBorderIntensity')" /><span>{{ draft.workspace_border_intensity }}%</span></div></NFormItem>
      <NFormItem :label="t('settings.workspaceBorderPreview')"><div class="border-controls"><div class="border-preview" :style="borderPreview">{{ t('settings.workspaceBorderPreview') }}</div><NButton @click="resetWorkspaceBorder">{{ t('settings.workspaceBorderReset') }}</NButton><span class="border-hint">{{ t('settings.workspaceBorderHint') }}</span></div></NFormItem>
      <NFormItem :label="t('settings.font')"><NInput v-model:value="draft.font_family" :maxlength="500" /></NFormItem>
      <NFormItem :label="t('settings.fontSize')"><NInputNumber v-model:value="draft.font_size" :min="8" :max="40" :precision="0" /></NFormItem>
      </fieldset><fieldset class="settings-section"><legend>{{ t('settings.terminal') }}</legend>
      <NFormItem :label="t('settings.scrollback')"><NInputNumber v-model:value="draft.scrollback" :min="0" :max="100000" :precision="0" /></NFormItem>
      <NFormItem>
        <template #label>
          <span class="shell-integration-label">{{ t('settings.shellIntegration') }}
            <NTooltip trigger="manual" :show="show && (shellHelpHovered || shellHelpFocused)" :style="{ maxWidth: 'min(340px, calc(100vw - 32px))' }">
              <template #trigger>
                <button type="button" class="shell-integration-help" :aria-label="t('settings.shellIntegrationHelp')" @mouseenter="shellHelpHovered = true" @mouseleave="shellHelpHovered = false" @focus="shellHelpFocused = true" @blur="shellHelpFocused = false">
                  <UiIcon name="info" :size="16" />
                </button>
              </template>
              <div class="shell-integration-description">
                <p>{{ t('settings.shellIntegrationBody1') }}</p>
                <p>{{ t('settings.shellIntegrationBody2') }}</p>
                <p>{{ t('settings.shellIntegrationBody3') }}</p>
              </div>
            </NTooltip>
          </span>
        </template>
        <NSwitch v-model:value="draft.shell_integration" />
      </NFormItem>
      </fieldset><fieldset class="settings-section"><legend>{{ t('settings.notifications') }}</legend>
      <NFormItem :label="t('settings.notifyThreshold')"><NInputNumber v-model:value="draft.notify_command_seconds" :min="0" :max="3600" :precision="0" /></NFormItem>
      </fieldset><fieldset class="settings-section"><legend>{{ t('settings.defaults') }}</legend>
      <NFormItem :label="t('settings.confirmClose')"><div><NSwitch v-model:value="draft.confirm_close_terminal" /><div>{{ t('settings.confirmCloseHint') }}</div></div></NFormItem>
      <NFormItem :label="t('settings.defaultShell')"><NSelect v-model:value="draft.default_shell" :options="shellOptions" /></NFormItem>
      <NFormItem :label="t('settings.defaultDirectory')"><div class="path-picker"><NInput v-model:value="draft.default_directory" clearable :placeholder="t('settings.defaultDirectoryPlaceholder')" /><NButton @click="chooseDirectory">{{ t('common.browse') }}</NButton></div></NFormItem>
      </fieldset><fieldset class="settings-section"><legend>{{ t('settings.shortcuts') }}</legend><NButton :disabled="saving || store.busy || childDialogOpen" @click="shortcutsDialog = true">{{ t('settings.shortcutSettings') }}</NButton></fieldset>
      <fieldset class="settings-section"><legend>{{ t('settings.about') }}</legend>
        <NFormItem :label="t('settings.version')"><span>Orbyte v{{ releaseSummary.version }}</span></NFormItem>
        <NButton :disabled="saving || childDialogOpen" @click="releaseDialog = true">{{ t('settings.releaseNotes') }}</NButton>
      </fieldset><NAlert v-if="error" type="error">{{ error }}</NAlert></div>
      <div class="dialog-actions"><NButton :disabled="saving || childDialogOpen" @click="emit('update:show', false)">{{ t('common.cancel') }}</NButton><NButton attr-type="submit" type="primary" :loading="saving" :disabled="childDialogOpen || !draft.font_family.trim() || draft.font_size == null || draft.scrollback == null || draft.notify_command_seconds == null || store.busy">{{ t('common.save') }}</NButton></div>
    </NForm>
  </NModal>
  <ShortcutSettingsDialog v-model:show="shortcutsDialog" />
  <ReleaseNotesDialog v-model:show="releaseDialog" />
</template>
<style scoped>
.border-intensity { display: flex; align-items: center; gap: 12px; width: 100%; }
.border-intensity > span { min-width: 40px; text-align: right; }
.border-controls { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; width: 100%; }
.border-preview { flex: 1; min-width: 100px; padding: 12px; border: 1px solid var(--workspace-border); border-radius: 9px; color: var(--text); background: var(--content); text-align: center; }
.border-hint { flex-basis: 100%; color: var(--muted); font-size: 12px; }
.shell-integration-label { display: inline-flex; align-items: center; gap: 4px; }
.shell-integration-help { display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; padding: 2px; border: 0; border-radius: 3px; background: transparent; color: var(--muted); cursor: help; }
.shell-integration-help:focus-visible { outline: 2px solid currentColor; outline-offset: 2px; }
.shell-integration-description p { margin: 0; }
.shell-integration-description p + p { margin-top: 8px; }
</style>
