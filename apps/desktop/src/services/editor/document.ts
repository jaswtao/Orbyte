/** 文档磁盘基线与未保存判定。Author: jaswtao · Date: 2026-10-13 */
export interface DocumentSnapshot { content: string; version: string }
export class DocumentState {
  content = ''
  savedContent = ''
  version = ''
  external = false
  get dirty() { return this.content !== this.savedContent }
  load(snapshot: DocumentSnapshot) { this.content = snapshot.content; this.savedContent = snapshot.content; this.version = snapshot.version; this.external = false }
  saved(snapshot: DocumentSnapshot) { this.savedContent = snapshot.content; this.version = snapshot.version; this.external = false }
  changed(snapshot: DocumentSnapshot): 'ignore' | 'reload' | 'conflict' {
    if (snapshot.version === this.version) return 'ignore'
    if (this.dirty) { this.external = true; return 'conflict' }
    return 'reload'
  }
}
export const unsavedCount = (documents: Iterable<{ dirty: boolean }>) => Array.from(documents).filter((document) => document.dirty).length
