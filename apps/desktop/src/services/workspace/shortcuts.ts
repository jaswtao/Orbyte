/** 应用快捷键配置、匹配与校验。Author: jaswtao · Date: 2026-10-15 */
import manifest from '../../../../../crates/orbyte-core/src/shortcuts.json'
import { commandLabel, setLocale, shortcutGroupLabel, t } from '../../i18n/translate'

export { setLocale }

export interface ShortcutKey {
  code: string
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  metaKey: boolean
}
export type ShortcutOverrides = Record<string, ShortcutKey[]>
export const shortcutCommands = manifest
export const shortcutScopes = ['project', 'workspace', 'terminal', 'editor'] as const
export { commandLabel, shortcutGroupLabel }
export function bindings(id: string, overrides: ShortcutOverrides = {}): ShortcutKey[] {
  return overrides[id] ?? shortcutCommands.find((command) => command.id === id)?.defaults ?? []
}
export function sameKey(a: ShortcutKey, b: ShortcutKey): boolean {
  return a.code === b.code && a.ctrlKey === b.ctrlKey && a.altKey === b.altKey && a.shiftKey === b.shiftKey && a.metaKey === b.metaKey
}
export function validShortcut(key: ShortcutKey): boolean {
  return /^(Key[A-Z]|Digit[0-9]|Numpad[0-9]|F([1-9]|1[0-2])|Arrow(Left|Right|Up|Down)|Tab|Space|Enter|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Backquote|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash)$/.test(key.code)
    && (key.ctrlKey || key.altKey || key.metaKey || /^F([1-9]|1[0-2])$/.test(key.code))
}
export function formatShortcut(key: ShortcutKey): string {
  const code = key.code.replace(/^Key|^Digit/, '').replace(/^Arrow/, '').replace(/^Numpad/, t('shortcut.numpad'))
  return [...(key.ctrlKey ? ['Ctrl'] : []), ...(key.altKey ? ['Alt'] : []), ...(key.shiftKey ? ['Shift'] : []), ...(key.metaKey ? ['Meta'] : []), code].join('+')
}
export function shortcutHint(id: string, overrides: ShortcutOverrides = {}): string {
  return bindings(id, overrides).map(formatShortcut).join(' / ')
}
export function shortcutConflicts(overrides: ShortcutOverrides): string[] {
  const errors: string[] = []
  for (const [index, command] of shortcutCommands.entries()) {
    const keys = bindings(command.id, overrides)
    const label = commandLabel(command.id)
    if (keys.some((key) => !validShortcut(key))) errors.push(t('shortcut.invalid', { label }))
    if (keys.some((key, i) => keys.slice(0, i).some((other) => sameKey(key, other)))) errors.push(t('shortcut.duplicate', { label }))
    for (const other of shortcutCommands.slice(0, index)) {
      if (command.scope !== other.scope && ['terminal', 'editor'].includes(command.scope) && ['terminal', 'editor'].includes(other.scope)) continue
      if (keys.some((key) => bindings(other.id, overrides).some((candidate) => sameKey(key, candidate)))) errors.push(t('shortcut.conflict', { label, other: commandLabel(other.id) }))
    }
  }
  return errors
}
export function matchShortcut(event: ShortcutKey & { isComposing?: boolean }, overrides: ShortcutOverrides = {}, scopes = ['project', 'workspace', 'terminal', 'editor']): string | undefined {
  if (event.isComposing) return
  return shortcutCommands.find((command) => scopes.includes(command.scope) && bindings(command.id, overrides).some((key) => sameKey(event, key)))?.id
}
