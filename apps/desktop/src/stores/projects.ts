/** Author: jaswtao · Date: 2026-10-12 */
import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  projectApi,
  type Project,
  type ProjectGroup,
  type Settings,
} from '../services/projectApi'

export const useProjects = defineStore('projects', () => {
  const projects = ref<Project[]>([])
  const groups = ref<ProjectGroup[]>([])
  const settings = ref<Settings | null>(null)
  const busy = ref(false)
  const error = ref('')
  const ready = ref(false)
  const active = computed(() =>
    projects.value.find((p) => p.id === settings.value?.active_project_id),
  )

  async function refresh() {
    const [nextProjects, nextGroups, nextSettings] = await Promise.all([
      projectApi.listProjects(),
      projectApi.listGroups(),
      projectApi.getSettings(),
    ])
    projects.value = nextProjects
    groups.value = nextGroups
    settings.value = nextSettings
    ready.value = true
  }

  // 串行修改，防止快速切换、拖拽和删除相互覆盖；失败保留原视图。
  async function run(action: () => Promise<unknown>): Promise<boolean> {
    if (busy.value) return false
    busy.value = true
    error.value = ''
    try {
      await action()
      await refresh()
      return true
    } catch (e) {
      error.value = String(e)
      return false
    } finally {
      busy.value = false
    }
  }

  function activate(id: string) {
    return run(() => projectApi.setSetting('active_project_id', id))
  }

  return {
    projects,
    groups,
    settings,
    busy,
    error,
    ready,
    active,
    refresh,
    run,
    activate,
  }
})
