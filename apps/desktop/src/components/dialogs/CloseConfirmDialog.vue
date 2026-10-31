<!-- Author: jaswtao · Date: 2026-10-14 -->
<script setup lang="ts">
import { NAlert, NButton, NModal } from 'naive-ui'
import { t } from '../../i18n'
defineProps<{ show: boolean; busy: boolean; error: string; unsaved: boolean; transfers: boolean }>()
const emit = defineEmits<{ resolve: [action: 'save' | 'discard' | 'exit' | 'cancel'] }>()
</script>
<template>
      <NModal :show="show" preset="card" :title="transfers && !unsaved ? t('close.transferTitle') : unsaved && transfers ? t('close.bothTitle') : t('close.title')" class="form-modal" :mask-closable="false" :closable="!busy" @update:show="(show) => { if (!show) emit('resolve', 'cancel') }">
        <p v-if="unsaved">{{ t('close.body') }}</p>
        <p v-if="transfers">{{ t('close.transferBody') }}</p>
        <NAlert v-if="error" type="error">{{ error }}</NAlert>
        <div class="dialog-actions">
          <NButton :disabled="busy" @click="emit('resolve', 'cancel')">{{ t('common.cancel') }}</NButton>
          <template v-if="unsaved">
            <NButton type="primary" :loading="busy" @click="emit('resolve', 'save')">{{ t('close.save') }}</NButton>
            <NButton type="error" :disabled="busy" @click="emit('resolve', 'discard')">{{ t('close.discard') }}</NButton>
          </template>
          <NButton v-else type="error" :disabled="busy" @click="emit('resolve', 'exit')">{{ t('close.exit') }}</NButton>
        </div>
      </NModal>
</template>
