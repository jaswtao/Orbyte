/** Tracks explicit interaction independently of automatic pane focus. Author: jaswtao · Date: 2026-10-30 */
export type RenameTarget = { kind: 'project'; id: string } | { kind: 'workspace' } | null

export function createRenameTargetTracker() {
  let target: RenameTarget = null
  let tabNavigation = false
  function region(node: EventTarget | null): RenameTarget {
    if (!(node instanceof Element)) return null
    if (node.closest('.n-modal-container, .n-dropdown-menu')) return null
    if (node.closest('input, textarea, [contenteditable="true"]') && !node.closest('.xterm, .cm-content')) return null
    const row = node.closest<HTMLElement>('.project-row')
    if (row?.dataset.projectId) return { kind: 'project', id: row.dataset.projectId }
    return node.closest('.workspace, .workspace-tabs') ? { kind: 'workspace' } : null
  }
  return {
    get target() { return target },
    pointerdown(event: PointerEvent) { tabNavigation = false; target = region(event.target) },
    focusin(event: FocusEvent) {
      const next = region(event.target)
      // Project activation moves focus into a pane without changing the user's target.
      if (tabNavigation || next?.kind !== 'workspace' || target?.kind !== 'project') target = next
      tabNavigation = false
    },
    keydown(event: KeyboardEvent, isRename: boolean) {
      tabNavigation = event.key === 'Tab'
      if (isRename || tabNavigation || ['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return
      target = region(event.target)
    },
    reset() { target = null; tabNavigation = false },
  }
}
