/** Author: jaswtao · Date: 2026-10-27 */
import type { Project } from '../projectApi'

export function filterProjects(projects: Project[], showHidden: boolean, favoritesOnly = false, search = '') {
  const query = search.toLocaleLowerCase()
  return projects.filter((project) =>
    (showHidden || !project.hidden) &&
    (!favoritesOnly || project.favorite) &&
    `${project.name} ${project.path}`.toLocaleLowerCase().includes(query),
  )
}

export function firstVisibleProject(projects: Project[], groupIds: string[], excludedId: string, favoritesOnly: boolean, search: string) {
  const candidates = filterProjects(projects, false, favoritesOnly, search)
  return groupIds.flatMap((id) => candidates.filter((project) => (project.groupId ?? '') === id))
    .find((project) => project.id !== excludedId)
}
