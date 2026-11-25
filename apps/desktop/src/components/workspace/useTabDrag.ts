/** Tab insertion previews. Author: jaswtao · Date: 2026-11-25 */
import { ref, watch } from 'vue'
import { moveTab, type Layout } from '../../services/workspace/layout'

type DropTarget = { id: string; side: 'before' | 'after' }

function tabTarget(event: DragEvent): DropTarget | null {
  const bar = event.currentTarget as HTMLElement
  const barBounds = bar.getBoundingClientRect()
  const { clientX: x, clientY: y } = event
  if (x < barBounds.left || x >= barBounds.right || y < barBounds.top || y >= barBounds.bottom) return null
  let previous: HTMLElement | undefined
  for (const tab of bar.querySelectorAll<HTMLElement>(':scope > [data-tab-id]')) {
    const bounds = tab.getBoundingClientRect()
    if (x < bounds.right) {
      return { id: tab.dataset.tabId!, side: x < bounds.left + bounds.width / 2 ? 'before' : 'after' }
    }
    previous = tab
  }
  const addButton = previous?.nextElementSibling
  if (previous && addButton?.matches('.workspace-tab-add') && x < addButton.getBoundingClientRect().left) {
    return { id: previous.dataset.tabId!, side: 'after' }
  }
  return null
}

export function useTabDrag(getLayout: () => Layout | null, enabled: () => boolean, save: (next: Layout) => void) {
  const draggingTab = ref<string | null>(null)
  const dropTarget = ref<DropTarget | null>(null)

  function endTabDrag() {
    draggingTab.value = null
    dropTarget.value = null
  }

  watch(enabled, (canDrag) => { if (!canDrag) endTabDrag() }, { flush: 'sync' })

  function startTabDrag(event: DragEvent, id: string) {
    endTabDrag()
    if (!enabled()) { event.preventDefault(); return }
    if (event.dataTransfer) {
      event.dataTransfer.setData('application/x-orbyte-workspace-tab', id)
      event.dataTransfer.effectAllowed = 'move'
    }
    draggingTab.value = id
  }

  function resolveDrop(event: DragEvent) {
    const source = draggingTab.value
    if (!source || !enabled()) return null
    const layout = getLayout()
    const target = tabTarget(event)
    if (!layout || !target) return null
    const next = moveTab(layout, source, target.id, target.side)
    return next === layout ? null : { target, next }
  }

  function overTabBar(event: DragEvent) {
    const target = resolveDrop(event)?.target ?? null
    if (dropTarget.value?.id !== target?.id || dropTarget.value?.side !== target?.side) dropTarget.value = target
    if (!target) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  }

  function leaveTabBar(event: DragEvent) {
    if (event.relatedTarget && (event.currentTarget as HTMLElement).contains(event.relatedTarget as Node)) return
    dropTarget.value = null
  }

  function dropTab(event: DragEvent) {
    const drop = resolveDrop(event)
    endTabDrag()
    if (!drop) return
    event.preventDefault()
    save(drop.next)
  }

  function tabDragClasses(id: string) {
    return {
      'workspace-tab-dragging': draggingTab.value === id,
      'workspace-tab-drop-before': dropTarget.value?.id === id && dropTarget.value.side === 'before',
      'workspace-tab-drop-after': dropTarget.value?.id === id && dropTarget.value.side === 'after',
    }
  }

  return { startTabDrag, endTabDrag, overTabBar, leaveTabBar, dropTab, tabDragClasses }
}
