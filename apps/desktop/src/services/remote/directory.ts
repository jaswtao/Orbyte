/** Parse remote directory reports without local host or Windows path conversion. Author: jaswtao · Date: 2026-10-31 */

export type RemotePrompt = 'unknown' | 'ready' | 'dirty' | 'busy'
export type RemotePromptEvent = 'cwd' | 'input' | 'submit'

/**
 * The session directory hook reports before each prompt. Reports can arrive
 * after typed input, so they must not make an unfinished line safe to overwrite.
 */
export function nextRemotePrompt(state: RemotePrompt, event: RemotePromptEvent): RemotePrompt {
  if (event === 'cwd') return state === 'dirty' ? 'dirty' : 'ready'
  if (event === 'submit') return 'busy'
  return 'dirty'
}

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
