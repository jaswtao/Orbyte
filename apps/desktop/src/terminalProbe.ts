/**
 * 步骤 1 验证页：创建终端、在终端与空白页之间切换（隐藏容器继续接收输出）、结束会话。
 *
 * Author: jaswtao
 * Date: 2026-10-09
 */
import '@xterm/xterm/css/xterm.css'
import './terminalProbe.css'

import { terminalApi, type ShellType } from './services/terminalApi'
import { INITIAL_SIZE, TerminalClient } from './services/terminal/terminalClient'
import { projectApi, type Project } from './services/projectApi'

interface Tab {
  key: string
  title: string
  pane: HTMLElement
  button: HTMLButtonElement
  client?: TerminalClient
  configId?: string
}

const app = document.querySelector<HTMLDivElement>('#app')!
app.innerHTML = `
  <header class="toolbar">
    <select id="project"></select>
    <select id="shell"></select>
    <button id="new">新建终端</button>
    <button id="stop">结束当前会话</button>
    <button id="restart">重新启动</button>
    <button id="remove">移除终端</button>
    <span id="tabs" class="tabs"></span>
    <span id="status" class="status"></span>
  </header>
  <main id="panes" class="panes"></main>
`

const shellSelect = app.querySelector<HTMLSelectElement>('#shell')!
const projectSelect = app.querySelector<HTMLSelectElement>('#project')!
let projects: Project[] = []
const tabsEl = app.querySelector<HTMLSpanElement>('#tabs')!
const panesEl = app.querySelector<HTMLElement>('#panes')!
const statusEl = app.querySelector<HTMLSpanElement>('#status')!
const tabs: Tab[] = []
let activeKey = ''

function addTab(key: string, title: string): Tab {
  const pane = document.createElement('div')
  pane.className = 'pane'
  panesEl.appendChild(pane)
  const button = document.createElement('button')
  button.textContent = title
  button.onclick = () => activate(key)
  tabsEl.appendChild(button)
  const tab: Tab = { key, title, pane, button }
  tabs.push(tab)
  return tab
}

function activate(key: string): void {
  activeKey = key
  for (const tab of tabs) {
    const active = tab.key === key
    tab.pane.style.display = active ? 'block' : 'none'
    tab.button.classList.toggle('active', active)
    tab.client?.setVisible(active)
    if (active && tab.client) {
      tab.client.refreshSize()
      tab.client.focus()
    }
  }
}

async function createTerminal(): Promise<void> {
  const project = projects.find((p) => p.id === projectSelect.value)
  if (!project) {
    statusEl.textContent = '请先在主界面添加项目，再返回验证页'
    return
  }
  const shell = shellSelect.value as ShellType
  const tab = addTab(`t${tabs.length}`, shellSelect.selectedOptions[0]?.text ?? shell)
  activate(tab.key)
  const client = new TerminalClient(
    tab.pane,
    { fontFamily: 'Cascadia Mono, Consolas, monospace', fontSize: 14 },
    {
      onStatus: (s) => {
        tab.button.textContent = s.state === 'exited' ? `${tab.title}（退出 ${s.exitCode ?? '-'}）` : tab.title
      },
      onError: (m) => (statusEl.textContent = m),
      onGap: (n) => (statusEl.textContent = `输出缺口：丢弃 ${n} 字节`),
    },
  )
  tab.client = client
  try {
    const result = await terminalApi.createTerminal(project.id, project.workspaceId, shell, '', client.measure() ?? INITIAL_SIZE, client.createChannel())
    tab.configId = result.terminalConfigId
    client.attach(result.terminalId)
    client.focus()
  } catch (e) {
    statusEl.textContent = String(e)
  }
}

async function init(): Promise<void> {
  addTab('blank', '空白页').pane.innerHTML = '<p class="blank">空白页：终端在后台继续运行。</p>'
  projects = await projectApi.listProjects()
  for (const project of projects) {
    const option = document.createElement('option')
    option.value = project.id
    option.textContent = project.name
    projectSelect.appendChild(option)
  }
  const shells = await terminalApi.listAvailableShells()
  for (const s of shells) {
    const option = document.createElement('option')
    option.value = s.shellType
    option.textContent = s.available ? s.description : `${s.description}（${s.unavailableReason}）`
    option.disabled = !s.available
    shellSelect.appendChild(option)
  }
  activate('blank')
}

app.querySelector<HTMLButtonElement>('#new')!.onclick = () => void createTerminal()
app.querySelector<HTMLButtonElement>('#stop')!.onclick = async () => {
  const id = tabs.find((t) => t.key === activeKey)?.client?.id
  if (!id) return
  try {
    const code = await terminalApi.stopTerminal(id)
    statusEl.textContent = `会话已结束，退出码 ${code ?? '-'}`
  } catch (e) {
    statusEl.textContent = String(e)
  }
}

app.querySelector<HTMLButtonElement>('#restart')!.onclick = async () => {
  const tab = tabs.find((t) => t.key === activeKey)
  if (!tab?.configId || !tab.client) return
  try {
    const id = await terminalApi.startTerminal(tab.configId, tab.client.measure() ?? INITIAL_SIZE, tab.client.createChannel())
    tab.client.term.reset()
    tab.client.attach(id)
    tab.client.focus()
  } catch (e) {
    statusEl.textContent = String(e)
  }
}

app.querySelector<HTMLButtonElement>('#remove')!.onclick = async () => {
  const tab = tabs.find((t) => t.key === activeKey)
  if (!tab?.configId) return
  try {
    await terminalApi.removeTerminal(tab.configId)
    tab.client?.dispose()
    tab.pane.remove()
    tab.button.remove()
    tabs.splice(tabs.indexOf(tab), 1)
    activate('blank')
  } catch (e) {
    statusEl.textContent = String(e)
  }
}

init().catch((e) => (statusEl.textContent = String(e)))
