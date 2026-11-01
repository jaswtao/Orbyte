/** Saved sudo password consent and ordered submission. Author: jaswtao · Date: 2026-11-01 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'

const { SudoInput } = await loadService('terminal/sudoInput')
const { InputQueue } = await loadService('terminal/inputQueue')
const { decodeFrame, FrameKind } = await loadService('terminal/frame')
const flush = () => new Promise((resolve) => setImmediate(resolve))
const enter = (extra = {}) => ({ type: 'keydown', key: 'Enter', keyCode: 13,
  preventDefault() { this.prevented = true }, stopPropagation() {}, ...extra })

test('only a physical unmodified Enter submits; repeated Enter waits for completion', () => {
  const prompts = [], sent = []
  const sudo = new SudoInput((available) => prompts.push(available))
  sudo.update('prompt')
  for (const extra of [{ isComposing: true }, { keyCode: 229 }, { type: 'keyup' }, { shiftKey: true }, { ctrlKey: true }, { altKey: true }, { metaKey: true }]) {
    assert.equal(sudo.key(enter(extra), () => assert.fail('unexpected submission')), true)
  }
  let done
  const key = enter()
  assert.equal(sudo.key(key, (id, complete) => { sent.push(id); done = complete }), false)
  assert.equal(key.prevented, true)
  assert.equal(prompts.at(-1), false)
  assert.equal(sudo.key(enter(), () => assert.fail('duplicate submission')), false)
  assert.deepEqual(sent, ['prompt'])
  done()
  assert.equal(sudo.key(enter(), () => assert.fail('consumed prompt')), true)
})

test('manual typing, paste and backspace require a new prompt before offering a password', () => {
  const sudo = new SudoInput(() => {})
  for (const data of ['password', '\r', '\x1b[200~password\r\x1b[201~', '\x7f', '\x03']) {
    sudo.update('prompt')
    // All xterm onData input, including paste, goes through this path.
    sudo.manualInput(data)
    assert.equal(sudo.key(enter(), () => assert.fail('manual input must not submit')), true)
  }
  sudo.update('new-prompt')
  assert.equal(sudo.key(enter(), () => {}), false)
})

test('old submission completion cannot clear a replacement session prompt', () => {
  const prompts = []
  const sudo = new SudoInput((available) => prompts.push(available))
  let done
  sudo.update('old')
  sudo.key(enter(), (_, complete) => { done = complete })
  sudo.reset()
  sudo.update('new')
  done()
  assert.equal(prompts.at(-1), true)
  assert.equal(sudo.key(enter(), (id) => assert.equal(id, 'new')), false)
})

test('backend submission stays between preceding input and later input', async () => {
  const sent = []
  const queue = new InputQueue()
  let firstDone, passwordDone
  queue.resume((data) => {
    sent.push(data)
    return data === 'first' ? new Promise((resolve) => { firstDone = resolve }) : Promise.resolve()
  })
  queue.push('first')
  queue.push('second')
  queue.pushAction(() => { sent.push('saved password'); return new Promise((resolve) => { passwordDone = resolve }) })
  queue.push('third')
  assert.deepEqual(sent, ['first'])
  firstDone()
  await flush()
  assert.deepEqual(sent, ['first', 'second', 'saved password'])
  passwordDone()
  await flush()
  assert.deepEqual(sent, ['first', 'second', 'saved password', 'third'])
})

test('a failed backend submission sends no replacement newline and queue pause drops queued actions', async () => {
  const sent = [], errors = []
  const queue = new InputQueue((error) => errors.push(error))
  queue.resume(async (data) => { sent.push(data) })
  queue.pushAction(async () => { throw new Error('missing credential') })
  await flush()
  assert.equal(errors.length, 1)
  assert.deepEqual(sent, [])
  queue.pause()
  queue.pushAction(async () => assert.fail('paused'))
  queue.resume(async () => {})
  await flush()
})

test('sudo metadata frames decode without containing terminal output', () => {
  const payload = new TextEncoder().encode(JSON.stringify({ promptId: 'prompt-id' }))
  const frame = new Uint8Array(17 + payload.length)
  frame[0] = FrameKind.SudoPrompt
  frame.set(payload, 17)
  assert.equal(decodeFrame(frame.buffer).kind, 5)
  assert.equal(decodeFrame(Array.from(frame)).kind, 5)
})
