<!-- Author: jaswtao · Date: 2026-10-16 -->
<script setup lang="ts">
import { watch } from 'vue'
import { useNotification } from 'naive-ui'
import { workspaceMessage } from '../services/workspace/messages'
import { t } from '../i18n'

const notification = useNotification()
// 同步消费每次事件，连续相同消息也不会因 Vue 的批处理被合并。
watch(workspaceMessage, (message) => {
  if (!message) return
  notification[message.type]({
    title: message.type === 'error' ? t('message.errorTitle') : t('terminal.warningTitle'),
    content: message.content,
    closable: true,
    duration: 6000,
    keepAliveOnHover: true,
  })
}, { flush: 'sync' })
</script>

<template />
