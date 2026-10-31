/** Remote projects and connection tabs. Author: jaswtao · Date: 2026-10-31 */
import { computed, reactive, ref } from 'vue'
import { defineStore } from 'pinia'
import { Channel } from '@tauri-apps/api/core'
import { remoteApi, type RemoteConnectionTab, type RemoteProject, type SshConnection, type TransferView } from '../services/remoteApi'
import { clearRemoteLink, markSftpReady } from '../services/remote/status'

export const useRemote = defineStore('remote', () => {
  const projects = ref<RemoteProject[]>([])
  const connections = ref<SshConnection[]>([])
  const tabs = ref<RemoteConnectionTab[]>([])
  const activeProjectId = ref<string | null>(null)
  const activeTabs = reactive(new Map<string, string>())
  const visited = reactive(new Set<string>())
  const transfers = ref<TransferView[]>([])
  let transferSubscription: Promise<void> | undefined
  const activeTabId = computed(() => activeProjectId.value ? activeTabs.get(activeProjectId.value) ?? null : null)
  const projectTabs = computed(() => tabs.value.filter((tab) => tab.projectId === activeProjectId.value))

  async function reload() {
    const [nextProjects, nextConnections, nextTabs] = await Promise.all([
      remoteApi.listProjects(), remoteApi.listConnections(), remoteApi.listConnectionTabs(),
    ])
    const changedConnections = new Set(nextConnections.filter((next) => {
      const previous = connections.value.find((connection) => connection.id === next.id)
      return previous && (previous.host !== next.host || previous.port !== next.port
        || previous.username !== next.username || previous.authType !== next.authType
        || previous.privateKeyPath !== next.privateKeyPath)
    }).map((connection) => connection.id))
    for (const tab of nextTabs) {
      if (changedConnections.has(tab.connectionId)) {
        visited.delete(tab.id)
        clearRemoteLink(tab.id)
        markSftpReady(tab.id, false)
      }
    }
    projects.value = nextProjects
    connections.value = nextConnections
    tabs.value = nextTabs
    if (!nextProjects.some((project) => project.id === activeProjectId.value)) activeProjectId.value = nextProjects[0]?.id ?? null
    for (const project of nextProjects) {
      const available = nextTabs.filter((tab) => tab.projectId === project.id)
      if (!available.some((tab) => tab.id === activeTabs.get(project.id))) {
        const first = available[0]
        if (first) activeTabs.set(project.id, first.id)
        else activeTabs.delete(project.id)
      }
    }
    for (const id of visited) {
      if (!nextTabs.some((tab) => tab.id === id)) {
        visited.delete(id)
        clearRemoteLink(id)
        markSftpReady(id, false)
      }
    }
  }
  function selectTab(id: string) {
    const tab = tabs.value.find((tab) => tab.id === id)
    if (!tab?.projectId) return
    activeProjectId.value = tab.projectId
    activeTabs.set(tab.projectId, id)
    visited.add(id)
  }
  function selectProject(id: string) {
    activeProjectId.value = id
    const tab = activeTabs.get(id)
    if (tab) selectTab(tab)
  }
  async function openConnection(connectionId: string) {
    const projectId = activeProjectId.value
    if (!projectId) return
    const workspace = await remoteApi.openConnectionTab(projectId, connectionId)
    tabs.value.push({ id: workspace.id, projectId, connectionId })
    selectTab(workspace.id)
  }
  async function closeTab(id: string) {
    await remoteApi.closeConnectionTab(id)
    await reload()
  }
  function listenTransfers(): Promise<void> {
    if (transferSubscription) return transferSubscription
    const channel = new Channel<{ transfers: TransferView[] }>()
    let live = false
    channel.onmessage = (event) => { live = true; transfers.value = event.transfers }
    transferSubscription = remoteApi.subscribeTransfers(channel).then(() => {}, (reason) => {
      transferSubscription = undefined
      throw reason
    })
    return transferSubscription.then(async () => {
      const snapshot = await remoteApi.listTransfers()
      if (!live) transfers.value = snapshot.transfers
    })
  }
  return { projects, connections, tabs, activeProjectId, activeTabId, projectTabs, visited, transfers, reload, selectTab, selectProject, openConnection, closeTab, listenTransfers }
})
