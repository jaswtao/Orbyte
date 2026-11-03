<!-- Author: jaswtao · Date: 2026-10-15 -->
<script setup lang="ts">
withDefaults(defineProps<{ name: 'folder' | 'folder-open' | 'folder-plus' | 'folder-upload' | 'file' | 'file-plus' | 'search' | 'settings' | 'chevron' | 'star' | 'terminal' | 'plus' | 'ellipsis' | 'info' | 'refresh' | 'close' | 'stop' | 'upload' | 'download' | 'arrow-up' | 'link' | 'panel-right' | 'eye'; size?: number }>(), { size: 18 })
const paths = {
  folder: 'M3 7V5a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Z',
  'folder-open': 'M3 18V5a1 1 0 0 1 1-1h5l2 2h8a1 1 0 0 1 1 1v2M3 18l2-8a1 1 0 0 1 1-.8h15l-2.3 9a1 1 0 0 1-1 .8H4a1 1 0 0 1-1-1Z',
  'folder-plus': 'M3 7V5a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Zm9 4v6m-3-3h6',
  'folder-upload': 'M3 7V5a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Zm9 10v-6m0 0-2.5 2.5M12 11l2.5 2.5',
  file: 'M6 3h8l4 4v14H6V3Zm8 0v5h4',
  'file-plus': 'M6 3h8l4 4v14H6V3Zm8 0v5h4M12 11v6m-3-3h6',
  search: 'm16 16 4 4M18 10.5a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0Z',
  chevron: 'm9 5 7 7-7 7',
  star: 'm12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9L12 3Z',
  terminal: 'm5 7 5 5-5 5m8 0h6M3 3h18v18H3V3Z',
  plus: 'M12 5v14M5 12h14',
  info: 'M12 8h.01M12 11v6M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0Z',
  refresh: 'M20 12a8 8 0 1 1-2.34-5.66M20 4v4h-4',
  close: 'm6 6 12 12M18 6 6 18',
  stop: 'M7 7h10v10H7z',
  upload: 'M12 15V4m0 0-4 4m4-4 4 4M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3',
  download: 'M12 4v11m0 0-4-4m4 4 4-4M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3',
  'arrow-up': 'M12 19V5m0 0-6 6m6-6 6 6',
  link: 'M9.5 14.5 14.5 9.5M8.5 12.5l-2 2a3.5 3.5 0 0 0 5 5l2-2M15.5 11.5l2-2a3.5 3.5 0 0 0-5-5l-2 2',
  'panel-right': 'M4 5h16v14H4V5Zm10 0v14',
  eye: 'M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Zm9.5 2.8a2.8 2.8 0 1 0 0-5.6 2.8 2.8 0 0 0 0 5.6Z',
}
</script>
<template>
  <svg :width="size" :height="size" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class="ui-icon">
    <template v-if="name === 'settings'"><path d="m9 3-.6 2.4-2 1.2L4 6l-2 3.5 1.8 1.7v2L2 15l2 3.5 2.4-.6 2 1.2L9 21h6l.6-1.9 2-1.2 2.4.6 2-3.5-1.8-1.8v-2L22 9.5 20 6l-2.4.6-2-1.2L15 3Z"/><circle cx="12" cy="12" r="3"/></template>
    <template v-else-if="name === 'ellipsis'"><circle cx="5.5" cy="12" r="1.3" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none"/><circle cx="18.5" cy="12" r="1.3" fill="currentColor" stroke="none"/></template>
    <path v-else :d="paths[name]" />
  </svg>
</template>
