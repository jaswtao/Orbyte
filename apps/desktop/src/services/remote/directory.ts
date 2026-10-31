/** Parse remote directory reports without local host or Windows path conversion. Author: jaswtao · Date: 2026-10-31 */
export function remoteDirectory(code: number, data: string): string | null {
  if (data.length > 8192 || /[\x00-\x1f\x7f-\x9f]/.test(data)) return null
  let path: string
  if (code === 9 && data.startsWith('9;')) path = data.slice(2)
  else if (code === 7) {
    try {
      const url = new URL(data)
      if (url.protocol !== 'file:') return null
      path = decodeURIComponent(url.pathname)
    } catch { return null }
  } else return null
  return path.startsWith('/') && !/[\x00-\x1f\x7f-\x9f]/.test(path) ? path : null
}
