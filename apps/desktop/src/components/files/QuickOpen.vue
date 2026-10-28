<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import { useProjects } from '../../stores/projects'
import { shortcutHint } from '../../services/workspace/shortcuts'
const settingsStore = useProjects()
const hint = (id: string) => shortcutHint(id, settingsStore.settings?.shortcuts)
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { NInput, NModal } from 'naive-ui'
import UiIcon from '../UiIcon.vue'
import { fileApi, fileError } from '../../services/fileApi'
import { t } from '../../i18n'
const props = defineProps<{ show: boolean; projectId: string; includeIgnored: boolean }>()
const emit = defineEmits<{ 'update:show': [show: boolean]; open: [path: string]; error: [message: string] }>()
const query = ref(''), paths = ref<string[]>([]), incomplete = ref(false), busy = ref(false), selected = ref(0)
const input = ref<{ focus: () => void }>()
let timer: ReturnType<typeof setTimeout> | undefined
let generation = 0
async function search(version: number) {
  busy.value = true
  try { const result = await fileApi.search(props.projectId, query.value, props.includeIgnored); if (version === generation) { paths.value = result.paths; incomplete.value = result.incomplete; selected.value = 0 } }
  catch (reason) { if (version === generation) emit('error', fileError(reason).message) }
  finally { if (version === generation) busy.value = false }
}
watch(() => [props.show, query.value, props.projectId, props.includeIgnored], async () => {
  clearTimeout(timer); generation++
  paths.value = []; incomplete.value = false; selected.value = 0; busy.value = false
  if (!props.show) return
  const version = generation
  timer = setTimeout(() => { void search(version) }, 100)
  await nextTick(); input.value?.focus()
})
function open(path: string) { emit('open', path); emit('update:show', false) }
function key(event: KeyboardEvent) {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); selected.value = Math.max(0, Math.min(paths.value.length - 1, selected.value + (event.key === 'ArrowDown' ? 1 : -1))) }
  if (event.key === 'Enter' && paths.value[selected.value]) open(paths.value[selected.value]!)
}
onBeforeUnmount(() => { generation++; clearTimeout(timer) })
</script>
<template>
  <NModal :show="show" preset="card" :title="t(includeIgnored ? 'quickOpen.ignoredTitle' : 'quickOpen.title', { hint: hint(includeIgnored ? 'quickOpen' : 'openFile') })" class="form-modal quick-open" @update:show="emit('update:show', $event)"><div @keydown="key"><NInput ref="input" v-model:value="query" :placeholder="t('quickOpen.placeholder')"><template #prefix><UiIcon name="search" :size="16" /></template></NInput><p v-if="busy">{{ t('quickOpen.searching') }}</p><p v-if="incomplete">{{ t('quickOpen.incomplete') }}</p><div class="file-rows quick-results"><button v-for="(path, index) in paths" :key="path" :class="{ selected: index === selected }" @click="open(path)">{{ path }}</button></div><div v-if="!busy && !paths.length" class="search-empty"><UiIcon name="search" :size="26" /><p>{{ t('quickOpen.empty') }}</p><small>{{ t('quickOpen.emptyHint') }}</small></div></div></NModal>
</template>
