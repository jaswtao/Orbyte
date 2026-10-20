/** 纯函数翻译，不依赖 Vue。Author: jaswtao · Date: 2026-10-16 */
import { en, type MessageKey } from './en'
import { zh } from './zh'

export type { MessageKey }
export type Locale = 'en' | 'zh'
export type Params = Record<string, string | number>

const catalogs: Record<Locale, Record<MessageKey, string>> = { en, zh }
let current: Locale = 'en'

export function getLocale(): Locale {
  return current
}

export function setLocale(locale: Locale) {
  current = locale
}

export function translate(locale: Locale, key: MessageKey, params?: Params): string {
  const text = catalogs[locale][key] ?? en[key]
  if (!params) return text
  return text.replace(/\{(\w+)\}/g, (placeholder, name: string) =>
    Object.hasOwn(params, name) ? String(params[name]) : placeholder)
}

export function t(key: MessageKey, params?: Params): string {
  return translate(current, key, params)
}

export function commandLabel(id: string): string {
  return t(`shortcut.command.${id}` as MessageKey)
}

export function shortcutGroupLabel(scope: string): string {
  return t(`shortcut.group.${scope}` as MessageKey)
}
