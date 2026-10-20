/** 命令完成通知；不包含终端内容。Author: jaswtao · Date: 2026-10-14 */
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification'
import { notificationAllowed } from './workspace/notificationPolicy'
import { lifecycleApi } from './lifecycleApi'
const sent = new Map<string, number>()
let permission: Promise<boolean> | undefined
// 应用退出会结束全部终端，这些退出不应提示。
let muted = false
void lifecycleApi.onShutdownStarted(() => { muted = true }).catch(() => {})
export async function notify(id: string, title: string, body: string) {
  const now = Date.now()
  if (muted || !notificationAllowed(sent.get(id), now)) return
  sent.set(id, now)
  permission ??= isPermissionGranted().then(async (granted) => granted || await requestPermission() === 'granted')
  try { if (await permission) await sendNotification({ title, body }) }
  catch { permission = undefined }
}
