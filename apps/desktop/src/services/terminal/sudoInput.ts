/** Saved-password input consent, independent of terminal output rendering.
 * Author: jaswtao · Date: 2026-11-01
 */
export class SudoInput {
  private promptId: string | null = null
  private submission: symbol | null = null

  constructor(private readonly onPrompt: (available: boolean) => void) {}

  update(promptId: string | null): void {
    this.promptId = promptId
    this.onPrompt(promptId !== null && this.submission === null)
  }

  reset(): void {
    this.submission = null
    this.update(null)
  }

  manualInput(): void { this.update(null) }

  key(event: KeyboardEvent, submit: (id: string, complete: () => void) => void): boolean {
    if (event.type !== 'keydown' || event.key !== 'Enter' || event.isComposing || event.keyCode === 229 ||
        event.ctrlKey || event.shiftKey || event.altKey || event.metaKey) return true
    if (!this.promptId && !this.submission) return true
    event.preventDefault()
    event.stopPropagation()
    if (this.submission) return false
    const id = this.promptId!
    const token = Symbol()
    this.submission = token
    this.update(null)
    submit(id, () => {
      if (this.submission !== token) return
      this.submission = null
      this.update(null)
    })
    return false
  }
}
