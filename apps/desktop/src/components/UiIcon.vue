<!-- Author: jaswtao · Date: 2026-10-15 -->
<script setup lang="ts">
withDefaults(defineProps<{ name: 'folder' | 'search' | 'settings' | 'chevron' | 'star' | 'terminal' | 'plus' | 'info'; size?: number }>(), { size: 18 })
const paths = {
  folder: 'M3 7V5a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Z',
  search: 'm16 16 4 4M18 10.5a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0Z',
  chevron: 'm9 5 7 7-7 7',
  star: 'm12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9L12 3Z',
  terminal: 'm5 7 5 5-5 5m8 0h6M3 3h18v18H3V3Z',
  plus: 'M12 5v14M5 12h14',
  info: 'M12 8h.01M12 11v6M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0Z',
}
</script>
<template>
  <svg :width="size" :height="size" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class="ui-icon">
    <template v-if="name === 'settings'"><path d="m9 3-.6 2.4-2 1.2L4 6l-2 3.5 1.8 1.7v2L2 15l2 3.5 2.4-.6 2 1.2L9 21h6l.6-1.9 2-1.2 2.4.6 2-3.5-1.8-1.8v-2L22 9.5 20 6l-2.4.6-2-1.2L15 3Z"/><circle cx="12" cy="12" r="3"/></template>
    <path v-else :d="paths[name]" />
  </svg>
</template>
