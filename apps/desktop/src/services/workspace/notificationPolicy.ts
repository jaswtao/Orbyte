/** 通知条件与频率边界。Author: jaswtao · Date: 2026-10-14 */
export function shouldNotify(threshold: number, duration: number | null, visible: boolean, focused: boolean): boolean {
  return threshold > 0 && (duration === null || duration >= threshold * 1000) && (!visible || !focused)
}
export function notificationAllowed(previous: number | undefined, now: number): boolean {
  return previous === undefined || now - previous >= 5000
}
