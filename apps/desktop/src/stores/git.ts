/** 仅查询本次已激活项目，按项目去抖和缓存。Author: jaswtao · Date: 2026-10-14 */
import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'
import { gitApi, type GitStatus } from '../services/gitApi'
export const useGit = defineStore('git', () => {
  const available = ref(false)
  const statuses = reactive(new Map<string, GitStatus>())
  const errors = reactive(new Map<string, string>())
  const visited = new Set<string>()
  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  const pending = new Map<string, Promise<void>>()
  const dirty = new Set<string>()
  let initialized: Promise<boolean> | undefined
  function init() { return initialized ??= gitApi.available().then((value) => available.value = value).catch(() => false) }
  async function refresh(id: string) {
    if (!visited.has(id) || !await init()) return
    if (pending.has(id)) { dirty.add(id); return pending.get(id) }
    const request = gitApi.status(id).then((status) => { statuses.set(id, status); errors.delete(id) }).catch((reason) => { errors.set(id, String(reason)) }).finally(() => { pending.delete(id); if (dirty.delete(id)) schedule(id) })
    pending.set(id, request)
    return request
  }
  function schedule(id: string) {
    if (!visited.has(id)) return
    clearTimeout(timers.get(id))
    timers.set(id, setTimeout(() => { timers.delete(id); void refresh(id) }, 1000))
  }
  function activate(id: string) { if (!visited.has(id)) { visited.add(id); void refresh(id) } }
  return { available, statuses, errors, init, activate, refresh, schedule }
})
