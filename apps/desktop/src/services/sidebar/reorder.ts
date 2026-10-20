/** 不修改输入集合的拖拽重排。Author: jaswtao · Date: 2026-10-14 */
export function dragPayload(kind: 'project' | 'group', id: string): string {
  return `${kind}:${id}`
}

/** 只认项目和分组；其余文本（例如外部文件拖入）忽略。 */
export function parseDragPayload(raw: string): { kind: 'project' | 'group'; id: string } | null {
  const split = raw.indexOf(':')
  if (split <= 0) return null
  const kind = raw.slice(0, split)
  const id = raw.slice(split + 1)
  if ((kind !== 'project' && kind !== 'group') || !id) return null
  return { kind, id }
}

export function reorderGroupIds(ids: string[], source: string, before: string): string[] {
  if (source === before || !ids.includes(source) || !ids.includes(before)) return [...ids]
  const next = ids.filter((id) => id !== source)
  next.splice(next.indexOf(before), 0, source)
  return next
}

export function reorderProjectEntries(projects: { id: string; groupId: string | null }[], source: string, group: string | null, before?: string): [string, string | null][] {
  const original: [string, string | null][] = projects.map((p) => [p.id, p.groupId])
  if (source === before || !projects.some((p) => p.id === source) || (before && !projects.some((p) => p.id === before))) return original
  const next = original.filter(([id]) => id !== source)
  const index = before ? next.findIndex(([id]) => id === before) : next.length
  next.splice(index, 0, [source, group])
  return next
}
