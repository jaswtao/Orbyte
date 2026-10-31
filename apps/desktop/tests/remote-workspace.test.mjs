/** Remote workspace rules that do not need a server. Author: jaswtao · Date: 2026-10-31 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { computed, nextTick, watch, markRaw, shallowReactive } from 'vue'

const {
  endpointLabel, remotePaneNeedsCloseConfirm, batchCancelId, transferPercent,
  parentPath, joinRemote, partitionSessions, failedRoots, formatBytes,
} = await loadService('remote/workspace')

test('a remote pane mounts a session bound after its first render and follows replacements', async () => {
  const source = ts.transpileModule(await readFile(new URL('../src/services/terminal/registry.ts', import.meta.url), 'utf8'), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText
  const exports = {}
  vm.runInNewContext(source, { exports, require(name) {
    if (name === 'vue') return { markRaw, shallowReactive }
    return {}
  } })
  const registry = new exports.TerminalRegistry(() => 'dark', () => ({}))
  const entry = computed(() => registry.byConfig.get('remote-config'))
  let mounted = null
  let focused = null
  const stop = watch(entry, (value) => { mounted = value?.host ?? null }, { immediate: true, flush: 'post' })
  const status = computed(() => entry.value?.status.state ?? 'not-started')
  const makeEntry = (id) => shallowReactive({
    host: { id }, status: { state: 'starting' },
    client: { attach() {}, focus() { focused = id } },
  })
  try {
    assert.equal(status.value, 'not-started')
    assert.equal(mounted, null)
    await Promise.resolve()
    const first = makeEntry('first')
    registry.bind('remote-config', first, 'ssh-session')
    await nextTick()
    assert.equal(mounted, first.host)
    first.status = { state: 'running' }
    assert.equal(status.value, 'running')
    entry.value.client.focus()
    assert.equal(focused, 'first')

    const replacement = makeEntry('replacement')
    registry.bind('remote-config', replacement, 'new-ssh-session')
    await nextTick()
    assert.equal(mounted, replacement.host)
    registry.byConfig.delete('remote-config')
    await nextTick()
    assert.equal(mounted, null)
    assert.equal(status.value, 'not-started')
  } finally { stop() }
})

test('remote panes always ask before closing, independent of the local confirm setting', () => {
  assert.equal(remotePaneNeedsCloseConfirm(), true)
  assert.equal(endpointLabel('root', 'example.com', 22), 'root@example.com:22')
  assert.equal(batchCancelId('abc'), 'batch:abc')
})

test('scanning and unknown totals are not shown as a percentage', () => {
  assert.equal(transferPercent('scanning', 10, null), null)
  assert.equal(transferPercent('queued', 0, 100), null)
  assert.equal(transferPercent('running', 50, null), null)
  assert.equal(transferPercent('running', 25, 100), 25)
  assert.equal(formatBytes(1536), '1.5 KB')
})

test('a remote session missing from the list is connected again and is not treated as exited', () => {
  const sessions = [{ terminalId: 'live', terminalConfigId: 'keep', status: { state: 'running' } }]
  const result = partitionSessions(['keep', 'gone'], sessions)
  assert.deepEqual(result.attach, [{ configId: 'keep', terminalId: 'live' }])
  assert.deepEqual(result.connect, ['gone'])
  assert.equal(parentPath('/var/log'), '/var')
  assert.equal(parentPath('/'), null)
  assert.equal(joinRemote('/var', 'log'), '/var/log')
  assert.deepEqual(failedRoots([{ relative: 'docs/a.txt', status: 'failed' }, { relative: 'docs/b.txt', status: 'skipped' }]), ['docs'])
})
