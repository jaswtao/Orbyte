<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import { NDropdown } from 'naive-ui'
const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ clickoutside: [] }>()
function escape(event: KeyboardEvent) { if (props.show && event.key === 'Escape') emit('clickoutside') }
onMounted(() => document.addEventListener('keydown', escape))
onBeforeUnmount(() => document.removeEventListener('keydown', escape))
</script>
<template><NDropdown v-bind="$attrs" :show="show" @clickoutside="emit('clickoutside')" /></template>
