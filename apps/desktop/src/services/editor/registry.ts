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
import { fileApi, fileError, type Encoding, type FileError, type TextFile, type EditorDraft } from '../fileApi'
import { t } from '../../i18n/translate'
import { DraftSync } from './drafts'
import { DocumentState, unsavedCount } from './document'

export interface EditorEntry {
  id: string; projectId: string; path: string; view: EditorView; document: DocumentState
  metadata: TextFile | null; error: string; deleted: boolean; busy: boolean
  initialized: boolean; recovery: EditorDraft | null; draftSync: DraftSync<EditorDraft> | null; draftError: string
  needsCheck: boolean
  dialog: FileError | { kind: 'reopen'; message: string; encoding: Encoding } | null
  editable: Compartment; style: Compartment; language: Compartment
}
export class EditorRegistry {
  readonly entries = shallowReactive(new Map<string, EditorEntry>())
  private readonly closingDrafts = new Map<string, Promise<void>>()
  private reports: Promise<void> = Promise.resolve()
  private lastCount = -1
  constructor(private readonly onError: (message: string) => void, private readonly settings: () => Settings, private readonly shortcutsBlocked: () => boolean = () => false, private readonly projectRoot: (id: string) => string = () => '') { this.report() }
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
    const style = new Compartment(), language = new Compartment(), editable = new Compartment()
    const view = markRaw(new EditorView({ state: EditorState.create({ extensions: [editable.of(EditorState.readOnly.of(true)), this.extensions(id, document, style.of(this.theme(settings)), language)] }) }))
    const entry = shallowReactive<EditorEntry>({ id, projectId, path, view, document, metadata: null, error: '', deleted: false, busy: false, initialized: false, recovery: null, draftSync: null, draftError: '', needsCheck: false, dialog: null, editable: markRaw(editable), style: markRaw(style), language: markRaw(language) })
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
    entry.view.setState(EditorState.create({ doc: file.content, extensions: [entry.editable.of(EditorState.readOnly.of(entry.busy || !entry.initialized || !!entry.recovery)), this.extensions(entry.id, entry.document, entry.style.of(entry.style.get(entry.view.state) ?? []), entry.language)] }))
    this.report()
  }
  private extensions(id: string, document: DocumentState, style: Extension, language: Compartment): Extension {
    return [basicSetup, style, language.of([]), Prec.highest(EditorView.domEventHandlers({ keydown: (event) => this.saveShortcut(id, event) })), EditorView.updateListener.of((update) => { if (update.docChanged) { document.content = update.state.doc.toString(); if (document.baselineKnown) document.recovered = false; this.entries.get(id)?.draftSync?.schedule(); this.report() } })]
  }
  private saveShortcut(id: string, event: KeyboardEvent): boolean {
    if (this.shortcutsBlocked() || matchShortcut(event, this.settings().shortcuts, ['editor']) !== 'editorSave') return false
    event.preventDefault()
    if (!event.repeat) void this.save(id)
    return true
  }
  async reload(id: string, encoding?: Encoding) {
    const entry = this.entries.get(id)
    if (!entry || entry.busy || (entry.initialized && entry.recovery)) return
    entry.busy = true
    entry.view.dispatch({ effects: entry.editable.reconfigure(EditorState.readOnly.of(true)) })
    const content = entry.document.content
    const initial = !entry.initialized
    try {
      await this.closingDrafts.get(`${entry.projectId}:${entry.path.toLowerCase()}`)
      if (this.entries.get(id) !== entry) return
      const file = await fileApi.read(entry.projectId, entry.path, encoding ?? entry.metadata?.encoding ?? null)
      if (this.entries.get(id) === entry) {
        if (entry.document.content !== content) { entry.error = t('editor.changedWhileReading'); return }
        this.apply(entry, file); void this.language(entry)
        if (!initial) await this.discard(id)
      }
    } catch (reason) { const error = fileError(reason); entry.error = error.message; entry.deleted = error.kind === 'missing' }
    finally {
      if (initial && this.entries.get(id) === entry) {
        try {
          const drafts = await fileApi.drafts()
          entry.recovery = drafts.find((d) => d.projectId === entry.projectId && d.path.toLowerCase() === entry.path.toLowerCase()) ?? null
          entry.initialized = true
          if (!entry.recovery) this.startDraftSync(entry)
        } catch (reason) { entry.draftError = t('draft.failed'); this.onError(String(reason)) }
      }
      if (entry.initialized && !entry.recovery) entry.view.dispatch({ effects: entry.editable.reconfigure(EditorState.readOnly.of(!entry.metadata)) })
      this.finish(entry)
    }
  }
  reopen(id: string, encoding: Encoding) {
    const entry = this.entries.get(id)
    if (!entry) return
    if (entry.document.dirty) entry.dialog = { kind: 'reopen', message: t('editor.reopenDirty'), encoding }
    else void this.reload(id, encoding)
  }
  async save(id: string, force = false, encoding?: Encoding) {
    const entry = this.entries.get(id)
    if (!entry || entry.busy || !entry.metadata || entry.recovery || !entry.initialized) return
    entry.busy = true
    const content = entry.view.state.doc.toString()
    try {
      const file = await fileApi.write(entry.projectId, entry.path, { ...entry.metadata, content, encoding: encoding ?? entry.metadata.encoding, expectedVersion: force ? null : entry.document.version })
      if (this.entries.get(id) !== entry) return
      entry.document.saved(file); entry.metadata = file; entry.error = ''; entry.deleted = false; entry.dialog = null; this.report()
      await entry.draftSync?.flush()
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
      if (entry.recovery || !entry.initialized) return
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
  private startDraftSync(entry: EditorEntry, initial: EditorDraft | null = null) {
    entry.draftError = ''
    entry.view.dispatch({ effects: entry.editable.reconfigure(EditorState.readOnly.of(!entry.metadata)) })
    entry.draftSync = markRaw(new DraftSync(() => entry.document.dirty && entry.metadata ? {
      projectId: entry.projectId, projectPath: this.projectRoot(entry.projectId), path: entry.path,
      content: entry.document.content, baseline: entry.document.version, encoding: entry.metadata.encoding,
      bom: entry.metadata.bom, lineEnding: entry.metadata.lineEnding, revision: crypto.randomUUID(), updatedAt: '',
    } : null, async (draft) => { await fileApi.putDraft(draft); entry.draftError = '' },
    async (draft) => { await fileApi.deleteDraft(draft.projectId, draft.path, draft.revision); entry.draftError = '' },
    (reason) => { entry.draftError = t(String(reason).includes('5 MiB') ? 'draft.tooLarge' : 'draft.failed'); this.onError(entry.draftError) }, initial))
  }
  async recover(id: string, restore: boolean) {
    const entry = this.entries.get(id), draft = entry?.recovery
    if (!entry || !draft || entry.busy) return
    if (restore && draft.projectPath.toLowerCase() !== this.projectRoot(entry.projectId).toLowerCase()) return
    entry.busy = true
    try {
      if (!restore) await fileApi.deleteDraft(draft.projectId, draft.path, draft.revision)
      else {
        const disk = entry.metadata
        // Keep the original version for optimistic save checks, even when disk content changed.
        const metadata: TextFile = { content: '', version: draft.baseline, encoding: draft.encoding, bom: draft.bom, lineEnding: draft.lineEnding }
        this.apply(entry, metadata)
        entry.view.dispatch({ changes: { from: 0, to: entry.view.state.doc.length, insert: draft.content } })
        entry.document.version = draft.baseline
        entry.document.external = !!disk && disk.version !== draft.baseline
        entry.deleted = !disk
        // An equal empty buffer is still a recovery copy until explicitly saved or discarded.
        entry.document.savedContent = disk?.version === draft.baseline ? disk.content : ''
        entry.document.baselineKnown = disk?.version === draft.baseline
        entry.document.recovered = true
      }
      entry.recovery = null
      if (!restore) this.apply(entry, entry.metadata ?? { content: '', version: '', encoding: draft.encoding, bom: draft.bom, lineEnding: draft.lineEnding })
      this.startDraftSync(entry, restore ? draft : null)
      this.report()
    } finally { this.finish(entry) }
  }
  rootChanged(entry: EditorEntry) { return !!entry.recovery && entry.recovery.projectPath.toLowerCase() !== this.projectRoot(entry.projectId).toLowerCase() }
  async deleteDraft(draft: EditorDraft) {
    const entry = Array.from(this.entries.values()).find((e) => e.projectId === draft.projectId && e.path.toLowerCase() === draft.path.toLowerCase())
    if (entry?.recovery?.revision === draft.revision) await this.recover(entry.id, false)
    else if (entry?.draftSync) await entry.draftSync.deleteDraft(draft)
    else await fileApi.deleteDraft(draft.projectId, draft.path, draft.revision)
  }
  async discard(id: string) {
    const entry = this.entries.get(id)
    if (!entry) return
    if (entry.recovery) await this.recover(id, false)
    await entry.draftSync?.discard()
  }
  async discardAll(projectId?: string) {
    for (const entry of this.entries.values()) if (!projectId || entry.projectId === projectId) await this.discard(entry.id)
  }
  async rename(projectId: string, from: string, to: string) {
    const entries = Array.from(this.entries.values()).filter((entry) => entry.projectId === projectId)
    await Promise.all(entries.map(async (entry) => {
      try { await entry.draftSync?.flush() } catch { /* Keep the last persisted copy and retry after migration. */ }
      await entry.draftSync?.pause()
    }))
    let migrated = false
    try {
      await fileApi.renameDrafts(projectId, from, to)
      migrated = true
    } catch { this.onError(t('draft.failed')) }
    finally {
      for (const entry of entries) {
        if (entry.path.toLowerCase() === from.toLowerCase() || entry.path.toLowerCase().startsWith(`${from.toLowerCase()}/`)) {
          entry.path = to + entry.path.slice(from.length)
          if (entry.recovery && migrated) entry.recovery = { ...entry.recovery, path: entry.path }
          void this.language(entry)
        }
        // On failure keep the old copy; a retry may leave an orphan but cannot delete recovery content.
        if (migrated) entry.draftSync?.relocate(entry.path)
        entry.draftSync?.resume()
        if (entry.document.dirty) entry.draftSync?.schedule()
      }
    }
  }
  remove(id: string) {
    const entry = this.entries.get(id)
    if (!entry) return
    const sync = entry.draftSync
    if (sync) {
      const key = `${entry.projectId}:${entry.path.toLowerCase()}`
      const pending = sync.flush().catch(() => {}).finally(() => {
        if (this.closingDrafts.get(key) === pending) this.closingDrafts.delete(key)
      })
      this.closingDrafts.set(key, pending)
      sync.dispose()
    }
    this.entries.delete(id); entry.view.destroy(); this.report()
  }
}
