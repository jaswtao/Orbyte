/** Remote projects and transport contexts. Author: jaswtao · Date: 2026-10-31 */
import { reactive, ref } from 'vue'
import { defineStore } from 'pinia'
import { Channel } from '@tauri-apps/api/core'
import { remoteApi, type RemoteConnectionTab, type RemoteProject, type SshConnection, type TransferView } from '../services/remoteApi'
import { clearRemoteLink, markSftpReady } from '../services/remote/status'

export const useRemote = defineStore('remote', () => {
  const projects = ref<RemoteProject[]>([])
  const connections = ref<SshConnection[]>([])
  const contexts = ref<RemoteConnectionTab[]>([])
  const activeProjectId = ref<string | null>(null)
  const visitedProjects = reactive(new Set<string>())
  const terminalCounts = reactive(new Map<string, number>())
  const revision = ref(0)
  const transfers = ref<TransferView[]>([])
  let transferSubscription: Promise<void> | undefined
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
        clearRemoteLink(tab.id)
        markSftpReady(tab.id, false)
      }
    }
    for (const id of visitedProjects) {
      if (!nextProjects.some((project) => project.id === id)) { visitedProjects.delete(id); terminalCounts.delete(id) }
    }
    revision.value += 1
    projects.value = nextProjects
    connections.value = nextConnections
    const contextsBeforeReload = contexts.value
    contexts.value = nextTabs
    if (!nextProjects.some((project) => project.id === activeProjectId.value)) activeProjectId.value = nextProjects[0]?.id ?? null
    for (const context of contextsBeforeReload) {
      if (!nextTabs.some((tab) => tab.id === context.id)) {
        clearRemoteLink(context.id)
        markSftpReady(context.id, false)
      }
    }
  }
  function selectProject(id: string) {
    if (projects.value.some((project) => project.id === id)) activeProjectId.value = id
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
  return { visitedProjects, terminalCounts, revision, projects, connections, activeProjectId, reload, selectProject, listenTransfers, transfers }
})
