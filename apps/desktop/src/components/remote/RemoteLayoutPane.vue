<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { LayoutNode } from '../../services/workspace/layout'
import type { TerminalRegistry } from '../../services/terminal/registry'
import { t, type MessageKey } from '../../i18n'

const props = defineProps<{
  node: LayoutNode
  path: number[]
  registry: TerminalRegistry
  names: Map<string, string>
  focused: string | null
  visible: boolean
  endpoint: string
}>()
const emit = defineEmits<{ focus: [id: string]; close: [id: string]; reconnect: [id: string]; ratio: [path: number[], value: number] }>()
const root = ref<HTMLElement>()
const mount = ref<HTMLElement>()
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
function attach() {
  if (mount.value && entry.value) {
    mount.value.appendChild(entry.value.host)
    if (props.visible) entry.value.client.refreshSize()
  }
}
onMounted(attach)
watch(() => [props.node, props.visible, entry.value], attach, { flush: 'post' })
function focus() {
  if (props.node.type !== 'terminal') return
  emit('focus', props.node.terminalConfigId)
  entry.value?.client.focus()
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
function label(key: MessageKey) { return t(key) }
</script>
<template>
  <div v-if="node.type === 'split'" ref="root" class="split-pane" :class="node.direction">
    <div class="split-child" :style="{ flex: `${node.ratio} 1 0%` }">
      <RemoteLayoutPane :node="node.children[0]" :path="[...path, 0]" :registry="registry" :names="names" :focused="focused" :visible="visible" :endpoint="endpoint" @focus="emit('focus', $event)" @close="emit('close', $event)" @reconnect="emit('reconnect', $event)" @ratio="(p, value) => emit('ratio', p, value)" />
    </div>
    <div class="split-handle" role="separator" :aria-orientation="node.direction === 'horizontal' ? 'vertical' : 'horizontal'" tabindex="0" @pointerdown="drag" @keydown="(e) => { if (node.type === 'split' && ['ArrowLeft', 'ArrowUp', 'ArrowRight', 'ArrowDown'].includes(e.key)) { e.preventDefault(); emit('ratio', path, node.ratio + (['ArrowLeft', 'ArrowUp'].includes(e.key) ? -0.05 : 0.05)) } }" />
    <div class="split-child" :style="{ flex: `${1 - node.ratio} 1 0%` }">
      <RemoteLayoutPane :node="node.children[1]" :path="[...path, 1]" :registry="registry" :names="names" :focused="focused" :visible="visible" :endpoint="endpoint" @focus="emit('focus', $event)" @close="emit('close', $event)" @reconnect="emit('reconnect', $event)" @ratio="(p, value) => emit('ratio', p, value)" />
    </div>
  </div>
  <div v-else-if="node.type === 'terminal'" class="terminal-pane" :class="{ focused: focused === node.terminalConfigId }" @mousedown.capture="focus">
    <div class="pane-heading">
      <span>{{ names.get(node.terminalConfigId) || label('remote.terminal') }} · {{ statusText }}</span>
      <button v-if="entry?.status.state === 'running' && !entry.displayPaused && entry.connection !== 'connected'" :disabled="entry.connection === 'reconnecting'" @click.stop="entry.client.reconnect()">{{ t(entry.connection === 'reconnecting' ? 'terminal.reconnecting' : 'terminal.reconnect') }}</button>
      <button v-if="entry && entry.status.state !== 'running' && entry.status.state !== 'starting'" @click.stop="emit('reconnect', node.terminalConfigId)">{{ t('remote.reconnect') }}</button>
      <button :aria-label="t('pane.close')" @click.stop="emit('close', node.terminalConfigId)">×</button>
    </div>
    <div v-if="entry?.displayPaused" class="terminal-output-warning" role="status">
      <span>{{ t(entry.status.state === 'running' ? 'terminal.displayPaused' : 'terminal.displayPausedEnded', { bytes: entry.droppedBytes.toLocaleString() }) }}</span>
      <button v-if="entry.status.state === 'running'" :disabled="entry.connection === 'reconnecting'" @click.stop="entry.client.resumeDisplay()">{{ t(entry.connection === 'reconnecting' ? 'terminal.reconnecting' : 'terminal.resumeDisplay') }}</button>
    </div>
    <div ref="mount" class="terminal-mount" />
    <div class="pane-status" :title="endpoint">{{ endpoint }}</div>
  </div>
</template>
