<!-- Author: jaswtao · Date: 2026-10-29 -->
<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NModal } from 'naive-ui'
import { locale, t } from '../i18n'
import { releaseSummary } from '../services/release'

defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [show: boolean] }>()
const notes = computed(() => releaseSummary[locale.value])
</script>

<template>
  <NModal :show="show" preset="card" :title="t('release.title')" class="form-modal release-modal" @update:show="emit('update:show', $event)">
    <div class="release-content">
      <p class="release-version">Orbyte v{{ releaseSummary.version }}</p>
      <p class="release-date">{{ t('release.date', { date: releaseSummary.date }) }}</p>
      <section v-if="notes.features.length">
        <h3>{{ t('release.features') }}</h3>
        <ul><li v-for="(item, index) in notes.features" :key="index">{{ item }}</li></ul>
      </section>
      <section v-if="notes.fixes.length">
        <h3>{{ t('release.fixes') }}</h3>
        <ul><li v-for="(item, index) in notes.fixes" :key="index">{{ item }}</li></ul>
      </section>
    </div>
    <div class="dialog-actions"><NButton @click="emit('update:show', false)">{{ t('common.close') }}</NButton></div>
  </NModal>
</template>

<style scoped>
.release-modal { width: 640px; }
.release-content { max-height: calc(100vh - 240px); overflow-y: auto; overflow-wrap: anywhere; padding-right: 12px; }
.release-version { margin: 0; font-size: 20px; font-weight: 600; }
.release-date { color: var(--muted); }
h3 { margin: 20px 0 8px; font-size: 15px; }
ul { margin: 0; padding-left: 22px; }
li + li { margin-top: 10px; }
</style>
