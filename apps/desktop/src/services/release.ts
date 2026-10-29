/** Bundled release summary for this application build. Author: jaswtao · Date: 2026-10-29 */
interface ReleaseNotes {
  features: string[]
  fixes: string[]
}

interface ReleaseSummary {
  version: string
  date: string
  en: ReleaseNotes
  zh: ReleaseNotes
}

declare const __ORBYTE_RELEASE__: ReleaseSummary

export const releaseSummary = __ORBYTE_RELEASE__
