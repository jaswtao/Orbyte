/** 布局树纯操作；终端实例不随树重建。Author: jaswtao · Date: 2026-10-13 */
export type Direction = 'horizontal' | 'vertical'
export type Leaf = { type: 'terminal'; terminalConfigId: string } | { type: 'editor'; id: string; path: string }
export type LayoutNode = Leaf | { type: 'split'; direction: Direction; ratio: number; children: [LayoutNode, LayoutNode] }
export interface WorkspaceTab { id: string; title: string; root: LayoutNode }
export interface Layout { schemaVersion: 1; activeTabId: string | null; tabs: WorkspaceTab[] }
export const emptyLayout = (): Layout => ({ schemaVersion: 1, activeTabId: null, tabs: [] })
export const leafId = (leaf: Leaf): string => leaf.type === 'terminal' ? leaf.terminalConfigId : leaf.id
export function leaves(node: LayoutNode): Leaf[] {
  return node.type === 'split' ? node.children.flatMap(leaves) : [node]
}
export function splitLeaf(node: LayoutNode, id: string, added: Leaf, direction: Direction): LayoutNode {
  if (node.type === 'split') return { ...node, children: node.children.map((child) => splitLeaf(child, id, added, direction)) as [LayoutNode, LayoutNode] }
  return leafId(node) === id ? { type: 'split', direction, ratio: 0.5, children: [node, added] } : node
}
export function removeLeaf(node: LayoutNode, id: string): LayoutNode | null {
  if (node.type !== 'split') return leafId(node) === id ? null : node
  const [left, right] = node.children.map((child) => removeLeaf(child, id))
  if (!left) return right ?? null
  if (!right) return left
  return { ...node, children: [left, right] }
}
export function removeFromLayout(layout: Layout, id: string): Layout {
  const tabs = layout.tabs.flatMap((tab) => {
    const root = removeLeaf(tab.root, id)
    return root ? [{ ...tab, root }] : []
  })
  return { ...layout, tabs, activeTabId: tabs.some((t) => t.id === layout.activeTabId) ? layout.activeTabId : tabs[0]?.id ?? null }
}
export function setRatio(node: LayoutNode, path: number[], ratio: number): LayoutNode {
  if (node.type !== 'split') return node
  if (!path.length) return { ...node, ratio: Math.max(0.1, Math.min(0.9, ratio)) }
  return { ...node, children: node.children.map((child, i) => i === path[0] ? setRatio(child, path.slice(1), ratio) : child) as [LayoutNode, LayoutNode] }
}
export function moveTab(layout: Layout, id: string, before: string): Layout {
  if (id === before) return layout
  const tab = layout.tabs.find((t) => t.id === id)
  if (!tab || !layout.tabs.some((t) => t.id === before)) return layout
  const tabs = layout.tabs.filter((t) => t.id !== id)
  tabs.splice(tabs.findIndex((t) => t.id === before), 0, tab)
  return { ...layout, tabs }
}
export function renameEditorPaths(layout: Layout, from: string, to: string): Layout {
  function rename(node: LayoutNode): LayoutNode {
    if (node.type === 'split') return { ...node, children: node.children.map(rename) as [LayoutNode, LayoutNode] }
    if (node.type === 'editor' && (node.path.toLowerCase() === from.toLowerCase() || node.path.toLowerCase().startsWith(`${from.toLowerCase()}/`))) return { ...node, path: to + node.path.slice(from.length) }
    return node
  }
  const name = (path: string) => path.split('/').pop() ?? path
  return { ...layout, tabs: layout.tabs.map((tab) => {
    const root = rename(tab.root)
    // 仅更新仍为默认文件名的单编辑器 Tab，保留用户自定义标题。
    const title = tab.root.type === 'editor' && root.type === 'editor' && tab.title === name(tab.root.path) ? name(root.path) : tab.title
    return { ...tab, root, title }
  }) }
}
