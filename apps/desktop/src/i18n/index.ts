/** 响应式语言。模板里的 t() 会在切换后重新渲染。Author: jaswtao · Date: 2026-10-16 */
import { ref } from 'vue'
import { en, type MessageKey } from './en'
import { setLocale as applyLocale, translate, type Locale, type Params } from './translate'

export const locale = ref<Locale>('en')

export function setLocale(value: Locale) {
  applyLocale(value)
  locale.value = value
}

export function t(key: MessageKey, params?: Params): string {
  return translate(locale.value, key, params)
}

export function commandLabel(id: string): string {
  return t(`shortcut.command.${id}` as MessageKey)
}

function reasonText(code: string): string {
  const key = `shell.reason.${code}` as MessageKey
  return key in en ? t(key) : code
}

export function shellLabel(shell: { description: string; available: boolean; unavailableReason: string | null }): string {
  const name = shell.description === 'cmd' ? t('shell.cmd') : shell.description
  if (shell.available || !shell.unavailableReason) return name
  return t('shell.unavailable', { name, reason: reasonText(shell.unavailableReason) })
}

export function presetLabel(preset: { description: string; available: boolean; unavailableReason: string | null }): string {
  if (preset.available || !preset.unavailableReason) return preset.description
  return t('shell.unavailable', { name: preset.description, reason: reasonText(preset.unavailableReason) })
}

export type { Locale, MessageKey, Params }
