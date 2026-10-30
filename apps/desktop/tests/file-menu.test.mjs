/** File context menu regression tests. Author: jaswtao · Date: 2026-10-30 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import vm from 'node:vm'
import ts from 'typescript'
import { computed, reactive, ref } from 'vue'

const source = await readFile(new URL('../src/components/files/FileTree.vue', import.meta.url), 'utf8')
const script = source.split('<script setup lang="ts">')[1].split('</script>')[0].replace(/^import .*$/gm, '')
const js = ts.transpileModule(script, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText
function fixture() {
  const calls = [], errors = [], props = { projectId: 'project', disabled: false, change: null }
  const context = {
    computed, reactive, ref, defineProps: () => props, defineEmits: () => (...args) => errors.push(args),
    useGit: () => ({ statuses: new Map() }), t: (key) => key,
    onMounted() {}, onBeforeUnmount() {}, watch() {},
    fileApi: { async reveal(...args) { calls.push(args); if (context.fail) throw new Error('Explorer failed') } },
    fileError: (reason) => ({ message: reason.message }),
  }
  vm.createContext(context)
  vm.runInContext(js, context)
  return { props, calls, errors, context, run: (code) => vm.runInContext(code, context) }
}

test('Right click uses viewport coordinates and selects the clicked row without opening it', () => {
  const f = fixture()
  f.run(`openMenu({ clientX: 310, clientY: 480 }, { path: 'src/file.ts', accessible: true })`)
  assert.equal(f.run('menu.value'), true)
  assert.equal(f.run('menuPosition.value.x'), 310)
  assert.equal(f.run('menuPosition.value.y'), 480)
  assert.equal(f.run('selected.value.path'), 'src/file.ts')
  assert.match(source, /:x="menuPosition.x" :y="menuPosition.y"/)
  assert.match(source, /@contextmenu.prevent.stop="openMenu\(\$event, entry\)"/)
  assert.deepEqual(f.errors, [])
})

test('Reveal targets the clicked file and blank space resets the target to the project root', async () => {
  const f = fixture()
  f.run(`openMenu({ clientX: 20, clientY: 30 }, { path: 'src/file.ts', accessible: true })`)
  await f.run(`selectMenu('reveal')`)
  assert.equal(f.run('menu.value'), false)
  f.run('openMenu({ clientX: 40, clientY: 50 })')
  assert.equal(f.run('selected.value'), null)
  assert.equal(f.run(`menuOptions.value.find(o => o.key === 'rename').disabled`), true)
  await f.run(`selectMenu('reveal')`)
  assert.deepEqual(f.calls, [['project', 'src/file.ts'], ['project', '']])
})

test('Disabled and inaccessible entries cannot launch Explorer; failures reach the error handler', async () => {
  const f = fixture()
  f.props.disabled = true
  f.run('openMenu({ clientX: 10, clientY: 20 })')
  assert.equal(f.run('menu.value'), false)
  await f.run(`selectMenu('reveal')`)
  f.props.disabled = false
  f.run(`openMenu({ clientX: 10, clientY: 20 }, { path: 'locked', accessible: false })`)
  assert.ok(f.run('menuOptions.value.every(o => o.disabled)'))
  await f.run(`selectMenu('reveal')`)
  assert.deepEqual(f.calls, [])
  f.context.fail = true
  f.run('openMenu({ clientX: 10, clientY: 20 })')
  await f.run(`selectMenu('reveal')`)
  assert.deepEqual(f.errors, [['error', 'Explorer failed']])
})

test('Escape dismisses the menu and rename keeps the clicked target', async () => {
  const f = fixture()
  f.run(`openMenu({ clientX: 10, clientY: 20 }, { path: 'src/file.ts', accessible: true })`)
  f.run(`escapeMenu({ key: 'Escape' })`)
  assert.equal(f.run('menu.value'), false)
  await f.run(`selectMenu('rename')`)
  assert.equal(f.run('dialog.value.original'), 'src/file.ts')
  assert.equal(f.run('dialog.value.operation'), 'rename')
})
