/** 主题解析与内容区一致性回归。Author: jaswtao · Date: 2026-10-15 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'

const { resolveAppearance, palettes, appearanceVariables, componentTheme, terminalAppearance } = await loadService('appearance')

test('显式主题优先于系统，跟随系统响应明暗切换', () => {
  assert.equal(resolveAppearance('dark', 'light'), 'dark')
  assert.equal(resolveAppearance('light', 'dark'), 'light')
  assert.equal(resolveAppearance('system', 'light'), 'light')
  assert.equal(resolveAppearance('system', 'dark'), 'dark')
  assert.equal(resolveAppearance('system', null), 'dark')
  assert.equal(resolveAppearance(undefined, null), 'dark')
})

test('两种主题的终端、CSS 和组件库保持配色一致', () => {
  for (const mode of ['light', 'dark']) {
    const css = appearanceVariables(mode)
    const terminal = terminalAppearance(mode)
    const components = componentTheme(mode)
    assert.equal(terminal.background, css['--content'])
    assert.equal(terminal.foreground, css['--text'])
    assert.equal(terminal.cursorAccent, terminal.background)
    assert.equal(terminal.selectionBackground, css['--terminal-selection'])
    assert.equal(components.common.primaryColor, css['--accent'])
    assert.equal(components.common.modalColor, palettes[mode].elevated)
    assert.notEqual(terminal.background, terminal.foreground)
  }
})
