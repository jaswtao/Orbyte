/** 生产入口分包依赖回归。Author: jaswtao · Date: 2026-10-15 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { build } from 'vite'

test('生产入口的静态依赖不能反向引用入口，避免组件初始化时白屏', async () => {
  const result = await build({
    root: fileURLToPath(new URL('../', import.meta.url)),
    logLevel: 'silent',
    build: { write: false },
  })
  const chunks = result.output.filter((output) => output.type === 'chunk')
  const entry = chunks.find((chunk) => chunk.isEntry)
  assert.ok(entry, '生产构建必须生成应用入口')
  const byName = new Map(chunks.map((chunk) => [chunk.fileName, chunk]))
  const visited = new Set()
  function visit(name, chain) {
    assert.notEqual(name, entry.fileName, `入口循环依赖：${[...chain, name].join(' -> ')}`)
    if (visited.has(name)) return
    visited.add(name)
    for (const dependency of byName.get(name)?.imports ?? []) visit(dependency, [...chain, name])
  }
  for (const dependency of entry.imports) visit(dependency, [entry.fileName])
})
