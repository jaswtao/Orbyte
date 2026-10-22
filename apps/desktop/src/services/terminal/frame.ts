/**
 * 终端 Channel 原始帧解码：`[kind: u8][seq: u64 LE][timestamp_ms: u64 LE][payload]`。
 *
 * Author: jaswtao
 * Date: 2026-10-09
 */

export const FrameKind = {
  Output: 1,
  Status: 2,
  Error: 3,
  Gap: 4,
} as const

export const FRAME_HEADER_LEN = 17

export type TerminalFrameData = ArrayBuffer | number[]

export interface TerminalFrame {
  kind: number
  seq: number
  timestampMs: number
  payload: Uint8Array
}

export function decodeFrame(data: TerminalFrameData): TerminalFrame {
  // Tauri's postMessage fallback serializes raw IPC responses as JSON byte arrays.
  // Small Channel frames still arrive as ArrayBuffer, so both can occur in one subscription.
  let buffer: ArrayBuffer
  if (Array.isArray(data)) {
    for (let index = 0; index < data.length; index++) {
      if (!Number.isInteger(data[index]) || data[index]! < 0 || data[index]! > 255) throw new Error('Invalid terminal frame byte')
    }
    buffer = new Uint8Array(data).buffer
  } else buffer = data
  if (!(buffer instanceof ArrayBuffer) || buffer.byteLength < FRAME_HEADER_LEN) throw new Error('Invalid terminal frame')
  const view = new DataView(buffer)
  if (![1, 2, 3, 4].includes(view.getUint8(0))) throw new Error('Unknown terminal frame kind')
  return {
    kind: view.getUint8(0),
    seq: Number(view.getBigUint64(1, true)),
    timestampMs: Number(view.getBigUint64(9, true)),
    payload: new Uint8Array(buffer, FRAME_HEADER_LEN),
  }
}

const decoder = new TextDecoder()

export function decodeJsonPayload<T>(payload: Uint8Array): T {
  return JSON.parse(decoder.decode(payload)) as T
}
