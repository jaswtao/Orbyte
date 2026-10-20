<!-- Author: jaswtao · Date: 2026-10-13 -->
<script setup lang="ts">
import { useProjects } from '../../stores/projects'
import { shortcutHint } from '../../services/workspace/shortcuts'
import { t } from '../../i18n'
const settingsStore = useProjects()
const hint = (id: string) => shortcutHint(id, settingsStore.settings?.shortcuts)
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { NDropdown } from 'naive-ui'
import type { LayoutNode } from '../../services/workspace/layout'
import type { TerminalConfig } from '../../services/terminalApi'
import type { TerminalRegistry } from '../../services/terminal/registry'
import type { EditorRegistry } from '../../services/editor/registry'
import EditorPane from '../editor/EditorPane.vue'

const props = defineProps<{ node: LayoutNode; path: number[]; registry: TerminalRegistry; editors: EditorRegistry; configs: Map<string, TerminalConfig>; focused: string | null; visible: boolean }>()
const emit = defineEmits<{ focus: [id: string]; close: [id: string]; restart: [id: string]; stop: [id: string]; export: [id: string]; ratio: [path: number[], value: number] }>()
const root = ref<HTMLElement>()
const mount = ref<HTMLElement>()
const moreMenuOpen = ref(false)
const config = computed(() => props.node.type === 'terminal' ? props.configs.get(props.node.terminalConfigId) : undefined)
const entry = computed(() => props.node.type === 'terminal' ? props.registry.byConfig.get(props.node.terminalConfigId) : undefined)
const statusText = computed(() => {
  const status = entry.value?.status
  if (!status) return t('pane.notStarted')
  switch (status.state) {
    case 'starting': return t('pane.starting')
    case 'running': return t('pane.running')
    case 'failed': return status.message
    case 'exited': return t('pane.exited', { code: status.exitCode ?? t('pane.unknownExit') })
  }
})
const directoryLabel = computed(() => (entry.value?.shell.integrated && entry.value.shell.cwd) || config.value?.cwd || t('pane.projectRoot'))
const moreMenuOptions = computed(() => [{ key: 'stop', label: t('pane.stop') }])
function selectMoreAction(key: string) {
  moreMenuOpen.value = false
  if (key === 'stop' && props.node.type === 'terminal' && entry.value?.status.state === 'running') {
    emit('stop', props.node.terminalConfigId)
  }
}
watch(() => [props.visible, entry.value?.status.state], () => { moreMenuOpen.value = false })
function attach() {
  if (mount.value && entry.value) {
    mount.value.appendChild(entry.value.host)
    if (props.visible) entry.value.client.refreshSize()
  }
}
onMounted(attach)
watch(() => [props.node, props.visible], attach, { flush: 'post' })
function focus() {
  if (props.node.type === 'terminal') {
    emit('focus', props.node.terminalConfigId)
    entry.value?.client.focus()
  }
}
let cleanupDrag: (() => void) | undefined
function drag(event: PointerEvent) {
  if (props.node.type !== 'split' || !root.value) return
  event.preventDefault()
  cleanupDrag?.()
  const horizontal = props.node.direction === 'horizontal'
  const bounds = root.value.getBoundingClientRect()
  const move = (e: PointerEvent) => emit('ratio', props.path, horizontal ? (e.clientX - bounds.left) / bounds.width : (e.clientY - bounds.top) / bounds.height)
  const end = () => { document.removeEventListener('pointermove', move); document.removeEventListener('pointerup', end); cleanupDrag = undefined }
  cleanupDrag = end
  document.addEventListener('pointermove', move)
  document.addEventListener('pointerup', end)
}
onBeforeUnmount(() => cleanupDrag?.())
</script>

<template>
  <div v-if="node.type === 'split'" ref="root" class="split-pane" :class="node.direction">
    <div class="split-child" :style="{ flex: `${node.ratio} 1 0%` }">
      <LayoutPane :node="node.children[0]" :path="[...path, 0]" :registry="registry" :editors="editors" :configs="configs" :focused="focused" :visible="visible" @focus="emit('focus', $event)" @close="emit('close', $event)" @restart="emit('restart', $event)" @stop="emit('stop', $event)" @export="emit('export', $event)" @ratio="(p, value) => emit('ratio', p, value)" />
    </div>
    <div class="split-handle" role="separator" :aria-orientation="node.direction === 'horizontal' ? 'vertical' : 'horizontal'" tabindex="0" @pointerdown="drag" @keydown="(e) => { if (node.type === 'split' && ['ArrowLeft', 'ArrowUp', 'ArrowRight', 'ArrowDown'].includes(e.key)) { e.preventDefault(); emit('ratio', path, node.ratio + (['ArrowLeft', 'ArrowUp'].includes(e.key) ? -0.05 : 0.05)) } }" />
    <div class="split-child" :style="{ flex: `${1 - node.ratio} 1 0%` }">
      <LayoutPane :node="node.children[1]" :path="[...path, 1]" :registry="registry" :editors="editors" :configs="configs" :focused="focused" :visible="visible" @focus="emit('focus', $event)" @close="emit('close', $event)" @restart="emit('restart', $event)" @stop="emit('stop', $event)" @export="emit('export', $event)" @ratio="(p, value) => emit('ratio', p, value)" />
    </div>
  </div>
  <div v-else-if="node.type === 'terminal'" class="terminal-pane" :class="{ focused: focused === node.terminalConfigId }" @mousedown.capture="focus">
    <div class="pane-heading">
      <span>{{ config?.launchPreset ? config.name : config?.shellType }} · {{ statusText }}</span>
      <button v-if="entry?.status.state === 'running' && entry.connection !== 'connected'" :disabled="entry.connection === 'reconnecting'" @click.stop="entry.client.reconnect()">{{ t(entry.connection === 'reconnecting' ? 'terminal.reconnecting' : 'terminal.reconnect') }}</button>
      <button v-if="entry?.status.state !== 'running' && entry?.status.state !== 'starting'" @click.stop="emit('restart', node.terminalConfigId)">{{ t('pane.restart') }}</button>
      <button :disabled="!entry" @click.stop="emit('export', node.terminalConfigId)">{{ t('pane.export') }}</button>
      <NDropdown v-if="entry?.status.state === 'running'" trigger="click" placement="bottom-end" :show="moreMenuOpen" :options="moreMenuOptions" @update:show="moreMenuOpen = $event" @select="selectMoreAction" @clickoutside="moreMenuOpen = false">
        <button :aria-label="t('pane.more')" :title="t('pane.more')" aria-haspopup="menu" :aria-expanded="moreMenuOpen" @click.stop>⋯</button>
      </NDropdown>
      <button :aria-label="t('pane.close')" :title="t('pane.closeTitle', { hint: hint('closePane') })" @click.stop="emit('close', node.terminalConfigId)">×</button>
    </div>
    <div ref="mount" class="terminal-mount" />
    <div class="pane-status" :title="directoryLabel" >{{ t('pane.directory', { path: directoryLabel }) }}</div>
  </div>
  <EditorPane v-else :id="node.id" :editors="editors" :visible="visible" :focused="focused === node.id" @focus="emit('focus', $event)" @close="emit('close', $event)" />
</template>
