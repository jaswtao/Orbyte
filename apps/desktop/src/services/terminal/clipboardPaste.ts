/** 把剪贴板图片路径格式化成当前 Shell 能打开的文本。Author: jaswtao · Date: 2026-10-15 */

export type PasteShell = 'powershell' | 'cmd' | 'wsl' | null

/** 剪贴板没有文本时 readText 会以 NotFoundError 拒绝，这不是粘贴失败。 */
export function clipboardHasNoText(error: unknown): boolean {
  return typeof error === 'object' && error !== null && 'name' in error && error.name === 'NotFoundError'
}

/** Windows 路径交给 WSL 时改成 /mnt/<盘符>，并用单引号避免变量展开。 */
export function formatClipboardPath(path: string, shellType: PasteShell): string {
  const slash = path.replace(/\\/g, '/')
  if (shellType !== 'wsl') return `"${slash}" `
  const drive = /^([A-Za-z]):\/(.*)$/.exec(slash)
  const wsl = drive ? `/mnt/${drive[1].toLowerCase()}/${drive[2]}` : slash
  return `'${wsl.replace(/'/g, `'\\''`)}' `
}
