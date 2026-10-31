<!-- Author: jaswtao · Date: 2026-10-31 -->
<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { NAlert, NButton, NForm, NFormItem, NInput, NInputNumber, NModal, NSelect } from 'naive-ui'
import { open } from '@tauri-apps/plugin-dialog'
import { remoteApi, type KnownHost, type SshConnection, type PasswordChange } from '../../services/remoteApi'
import { t } from '../../i18n'

const props = defineProps<{ connection: SshConnection | null }>()
const show = defineModel<boolean>('show', { required: true })
const emit = defineEmits<{ saved: []; error: [message: string] }>()
const form = reactive({ name: '', host: '', port: 22 as number | null, username: '', authType: 'password' as 'password' | 'privateKey', privateKeyPath: '' })
const password = ref('')
const clearPassword = ref(false)
const knownHosts = ref<KnownHost[]>([])
const busy = ref(false)
const error = ref('')
const authOptions = computed(() => [
  { label: t('remote.password'), value: 'password' },
  { label: t('remote.privateKey'), value: 'privateKey' },
])
watch(show, (visible) => {
  password.value = ''
  clearPassword.value = false
  if (!visible) return
  error.value = ''
  const current = props.connection
  Object.assign(form, current
    ? { name: current.name, host: current.host, port: current.port, username: current.username, authType: current.authType, privateKeyPath: current.privateKeyPath ?? '' }
    : { name: '', host: '', port: 22, username: '', authType: 'password', privateKeyPath: '' })
  void refreshKeys()
})
watch(() => form.authType, () => { password.value = '' })
async function refreshKeys() {
  knownHosts.value = []
  if (!show.value || !form.host.trim() || !form.port) return
  try { knownHosts.value = await remoteApi.listKnownHosts(form.host.trim(), form.port) }
  catch (reason) { error.value = String(reason) }
}
async function chooseKey() {
  try {
    const picked = await open({ multiple: false, directory: false, title: t('remote.chooseKey') })
    if (typeof picked === 'string') form.privateKeyPath = picked
  } catch (reason) { error.value = String(reason) }
}
async function forget(id: string) {
  try {
    await remoteApi.forgetHostKey(id)
    await refreshKeys()
  } catch (reason) { error.value = String(reason) }
}
async function save() {
  if (busy.value) return
  busy.value = true
  error.value = ''
  const input = {
    name: form.name.trim(),
    host: form.host.trim(),
    port: form.port ?? 22,
    username: form.username.trim(),
    authType: form.authType,
    privateKeyPath: form.authType === 'privateKey' ? form.privateKeyPath.trim() : null,
    passwordChange: (form.authType === 'password' && password.value
      ? { operation: 'set', password: password.value }
      : { operation: clearPassword.value ? 'clear' : 'keep' }) as PasswordChange,
  }
  try {
    if (props.connection) await remoteApi.updateConnection(props.connection.id, input)
    else await remoteApi.createConnection(input)
    show.value = false
    emit('saved')
  } catch (reason) {
    error.value = String(reason)
    emit('error', error.value)
  } finally {
    password.value = ''
    if (input.passwordChange.operation === 'set') input.passwordChange.password = ''
    busy.value = false
  }
}
</script>
<template>
  <NModal v-model:show="show" preset="card" :title="connection ? t('remote.edit') : t('remote.add')" class="form-modal">
    <NForm @submit.prevent="save">
      <NFormItem :label="t('remote.name')"><NInput v-model:value="form.name" :maxlength="100" /></NFormItem>
      <NFormItem :label="t('remote.host')"><NInput v-model:value="form.host" @change="refreshKeys" /></NFormItem>
      <NFormItem :label="t('remote.port')"><NInputNumber v-model:value="form.port" :min="1" :max="65535" @update:value="refreshKeys" /></NFormItem>
      <NFormItem :label="t('remote.username')"><NInput v-model:value="form.username" :maxlength="128" /></NFormItem>
      <NFormItem :label="t('remote.auth')"><NSelect v-model:value="form.authType" :options="authOptions" /></NFormItem>
      <NFormItem v-if="form.authType === 'password'" :label="t('remote.password')">
        <div class="path-picker">
          <NInput v-model:value="password" type="password" show-password-on="click" :placeholder="t(connection?.hasSavedPassword && !clearPassword ? 'remote.passwordSaved' : 'remote.passwordOptional')" :disabled="busy" />
          <NButton v-if="connection?.hasSavedPassword && !clearPassword" :disabled="busy" @click="clearPassword = true; password = ''">{{ t('remote.clearPassword') }}</NButton>
        </div>
      </NFormItem>
      <p v-if="form.authType === 'password'">{{ t(clearPassword ? 'remote.passwordWillClear' : 'remote.passwordStorage') }}</p>
      <NFormItem v-if="form.authType === 'privateKey'" :label="t('remote.keyPath')">
        <div class="path-picker">
          <NInput v-model:value="form.privateKeyPath" />
          <NButton @click="chooseKey">{{ t('remote.chooseKey') }}</NButton>
        </div>
      </NFormItem>
      <div v-if="connection" class="remote-keys">
        <strong>{{ t('remote.knownHosts') }}</strong>
        <p v-if="!knownHosts.length">{{ t('remote.noKeys') }}</p>
        <div v-for="key in knownHosts" :key="key.id" class="remote-key">
          <span>{{ key.algorithm }} · {{ key.fingerprintSha256 }}</span>
          <NButton size="tiny" @click="forget(key.id)">{{ t('remote.forgetKey') }}</NButton>
        </div>
      </div>
      <NAlert v-if="error" type="error">{{ error }}</NAlert>
      <div class="dialog-actions">
        <NButton :disabled="busy" @click="show = false">{{ t('common.cancel') }}</NButton>
        <NButton attr-type="submit" type="primary" :loading="busy" :disabled="!form.name.trim() || !form.host.trim() || !form.username.trim() || !form.port || (form.authType === 'privateKey' && !form.privateKeyPath.trim())">{{ t('common.save') }}</NButton>
      </div>
    </NForm>
  </NModal>
</template>
