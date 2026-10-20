/** 按窗格保留 CodeMirror，移动 DOM 时保留撤销历史。Author: jaswtao · Date: 2026-10-13 */
import { basicSetup } from 'codemirror'
import { matchShortcut } from '../workspace/shortcuts'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { tags } from '@lezer/highlight'
import { palettes, resolveAppearance, type AppearanceMode } from '../appearance'
import { Compartment, EditorState, Prec, type Extension } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { markRaw, shallowReactive } from 'vue'
import type { Settings } from '../projectApi'
import { fileApi, fileError, type Encoding, type FileError, type TextFile } from '../fileApi'
import { t } from '../../i18n/translate'
import { DocumentState, unsavedCount } from './document'

export interface EditorEntry {
  id: string; projectId: string; path: string; view: EditorView; document: DocumentState
  metadata: TextFile | null; error: string; deleted: boolean; busy: boolean
  needsCheck: boolean
  dialog: FileError | { kind: 'reopen'; message: string; encoding: Encoding } | null
  style: Compartment; language: Compartment
}
export class EditorRegistry {
  readonly entries = shallowReactive(new Map<string, EditorEntry>())
  private reports: Promise<void> = Promise.resolve()
  private lastCount = -1
  constructor(private readonly onError: (message: string) => void, private readonly settings: () => Settings, private readonly shortcutsBlocked: () => boolean = () => false) { this.report() }
  report() {
    const count = unsavedCount(Array.from(this.entries.values(), (e) => e.document))
    if (count === this.lastCount) return
    this.lastCount = count
    this.reports = this.reports.catch(() => {}).then(() => fileApi.unsaved(count)).catch((reason) => this.onError(String(reason)))
  }
  create(id: string, projectId: string, path: string, settings: Settings): EditorEntry {
    const existing = this.entries.get(id)
    if (existing) return existing
    const document = shallowReactive(new DocumentState())
    const style = new Compartment(), language = new Compartment()
    const view = markRaw(new EditorView({ state: EditorState.create({ extensions: [EditorState.readOnly.of(true), this.extensions(id, document, style.of(this.theme(settings)), language)] }) }))
    const entry = shallowReactive<EditorEntry>({ id, projectId, path, view, document, metadata: null, error: '', deleted: false, busy: false, needsCheck: false, dialog: null, style: markRaw(style), language: markRaw(language) })
    this.entries.set(id, entry)
    void this.reload(id)
    void this.language(entry)
    return entry
  }
  private theme(settings: Settings, mode: AppearanceMode = resolveAppearance(settings.theme, window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')) {
    const p = palettes[mode]
    const syntax = mode === 'light'
      ? { keyword: '#9B2393', string: '#B3261E', number: '#1750EB', comment: '#6E6E73', function: '#326D74', type: '#6C36A8' }
      : { keyword: '#FC83BD', string: '#FC8C83', number: '#D0BF69', comment: '#A1A1A6', function: '#67B7A4', type: '#C5A3FF' }
    return [EditorView.theme({
      '&': { height: '100%', color: p.text, backgroundColor: p.content },
      '.cm-content': { fontFamily: settings.font_family, fontSize: `${settings.font_size}px`, padding: '8px 0', caretColor: p.text },
      '.cm-scroller': { overflow: 'auto', fontFamily: settings.font_family },
      '.cm-gutters': { backgroundColor: p.content, color: p.muted, borderColor: p.border },
      '.cm-cursor, .cm-dropCursor': { borderLeftColor: p.text },
      '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: p.hover },
      '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': { backgroundColor: p.terminalSelection },
      '&.cm-focused': { outline: 'none' },
      '.cm-panels, .cm-tooltip': { backgroundColor: p.elevated, color: p.text, borderColor: p.border },
      '.cm-searchMatch': { backgroundColor: p.selection, outline: `1px solid ${p.accent}` },
      '.cm-matchingBracket': { backgroundColor: p.selection },
    }, { dark: mode === 'dark' }), syntaxHighlighting(HighlightStyle.define([
      { tag: tags.keyword, color: syntax.keyword },
      { tag: [tags.string, tags.regexp], color: syntax.string },
      { tag: [tags.number, tags.bool, tags.null], color: syntax.number },
      { tag: tags.comment, color: syntax.comment },
      { tag: tags.function(tags.variableName), color: syntax.function },
      { tag: [tags.typeName, tags.className], color: syntax.type },
      { tag: tags.heading, color: p.accent, fontWeight: '600' },
      { tag: tags.link, color: p.accent },
    ]))]
  }
  updateSettings(settings: Settings, mode: AppearanceMode) { for (const entry of this.entries.values()) entry.view.dispatch({ effects: entry.style.reconfigure(this.theme(settings, mode)) }) }
  private async language(entry: EditorEntry) {
    const ext = entry.path.split('.').pop()?.toLowerCase()
    try {
      const support = ext && ['js', 'jsx', 'ts', 'tsx', 'vue'].includes(ext) ? (await import('@codemirror/lang-javascript')).javascript({ typescript: ['ts', 'tsx'].includes(ext), jsx: ['jsx', 'tsx'].includes(ext) })
        : ext === 'json' ? (await import('@codemirror/lang-json')).json()
        : ext === 'css' ? (await import('@codemirror/lang-css')).css()
        : ext === 'html' ? (await import('@codemirror/lang-html')).html()
        : ext === 'py' ? (await import('@codemirror/lang-python')).python()
        : ext === 'rs' ? (await import('@codemirror/lang-rust')).rust()
        : ext === 'sql' ? (await import('@codemirror/lang-sql')).sql()
        : ext === 'md' ? (await import('@codemirror/lang-markdown')).markdown() : []
      if (this.entries.get(entry.id) === entry) entry.view.dispatch({ effects: entry.language.reconfigure(support) })
    } catch (reason) { this.onError(t('editor.highlightFailed', { reason: String(reason) })) }
  }
  private apply(entry: EditorEntry, file: TextFile) {
    entry.document.load(file)
    entry.metadata = file; entry.deleted = false; entry.error = ''
    // 重新读取建立新的撤销历史，防止撤销恢复已放弃的修改。
    entry.view.setState(EditorState.create({ doc: file.content, extensions: this.extensions(entry.id, entry.document, entry.style.of(entry.style.get(entry.view.state) ?? []), entry.language) }))
    this.report()
  }
  private extensions(id: string, document: DocumentState, style: Extension, language: Compartment): Extension {
    return [basicSetup, style, language.of([]), Prec.highest(EditorView.domEventHandlers({ keydown: (event) => this.saveShortcut(id, event) })), EditorView.updateListener.of((update) => { if (update.docChanged) { document.content = update.state.doc.toString(); this.report() } })]
  }
  private saveShortcut(id: string, event: KeyboardEvent): boolean {
    if (this.shortcutsBlocked() || matchShortcut(event, this.settings().shortcuts, ['editor']) !== 'editorSave') return false
    event.preventDefault()
    if (!event.repeat) void this.save(id)
    return true
  }
  async reload(id: string, encoding?: Encoding) {
    const entry = this.entries.get(id)
    if (!entry || entry.busy) return
    entry.busy = true
    const content = entry.document.content
    try {
      const file = await fileApi.read(entry.projectId, entry.path, encoding ?? entry.metadata?.encoding ?? null)
      if (this.entries.get(id) === entry) {
        if (entry.document.content !== content) { entry.error = t('editor.changedWhileReading'); return }
        this.apply(entry, file); void this.language(entry)
      }
    } catch (reason) { const error = fileError(reason); entry.error = error.message; entry.deleted = error.kind === 'missing' }
    finally { this.finish(entry) }
  }
  reopen(id: string, encoding: Encoding) {
    const entry = this.entries.get(id)
    if (!entry) return
    if (entry.document.dirty) entry.dialog = { kind: 'reopen', message: t('editor.reopenDirty'), encoding }
    else void this.reload(id, encoding)
  }
  async save(id: string, force = false, encoding?: Encoding) {
    const entry = this.entries.get(id)
    if (!entry || entry.busy || !entry.metadata) return
    entry.busy = true
    const content = entry.view.state.doc.toString()
    try {
      const file = await fileApi.write(entry.projectId, entry.path, { ...entry.metadata, content, encoding: encoding ?? entry.metadata.encoding, expectedVersion: force ? null : entry.document.version })
      if (this.entries.get(id) !== entry) return
      entry.document.saved(file); entry.metadata = file; entry.error = ''; entry.deleted = false; entry.dialog = null; this.report()
    } catch (reason) {
      const error = fileError(reason)
      if (error.kind === 'message' || error.kind === 'missing') entry.error = error.message
      else { entry.dialog = error; if (error.position !== undefined) entry.view.dispatch({ selection: { anchor: error.position, head: Math.min(content.length, error.position + (error.length ?? 1)) }, scrollIntoView: true }) }
    } finally { this.finish(entry) }
  }
  private finish(entry: EditorEntry) {
    entry.busy = false
    if (entry.needsCheck && this.entries.get(entry.id) === entry) { entry.needsCheck = false; void this.external(entry.projectId, [entry.path]) }
  }
  async external(projectId: string, paths?: string[]) {
    await Promise.all(Array.from(this.entries.values()).filter((entry) => entry.projectId === projectId && (!paths || paths.some((p) => p.toLowerCase() === entry.path.toLowerCase() || entry.path.toLowerCase().startsWith(`${p.toLowerCase()}/`)))).map(async (entry) => {
      if (entry.busy) { entry.needsCheck = true; return }
      const content = entry.document.content
      const version = entry.document.version
      try {
        const file = await fileApi.read(projectId, entry.path, entry.metadata?.encoding ?? null)
        if (this.entries.get(entry.id) !== entry || entry.busy || entry.document.version !== version) return
        if (entry.document.content !== content) { entry.document.external = file.version !== version; return }
        if (entry.document.changed(file) === 'reload') { this.apply(entry, file); void this.language(entry) }
        entry.deleted = false
      } catch (reason) { if (this.entries.get(entry.id) === entry) { const error = fileError(reason); entry.deleted = error.kind === 'missing'; entry.error = error.message } }
    }))
  }
  rename(projectId: string, from: string, to: string) {
    for (const entry of this.entries.values()) if (entry.projectId === projectId && (entry.path.toLowerCase() === from.toLowerCase() || entry.path.toLowerCase().startsWith(`${from.toLowerCase()}/`))) { entry.path = to + entry.path.slice(from.length); void this.language(entry) }
  }
  remove(id: string) { const entry = this.entries.get(id); if (entry) { this.entries.delete(id); entry.view.destroy(); this.report() } }
}
