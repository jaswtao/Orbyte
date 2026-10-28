/** 主题解析与内容区一致性回归。Author: jaswtao · Date: 2026-10-15 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'

const { resolveAppearance, palettes, appearanceVariables, componentTheme, terminalAppearance } = await loadService('appearance')

test('workspace border defaults follow the theme and custom intensity affects only the border', () => {
  for (const mode of ['light', 'dark']) {
    assert.equal(appearanceVariables(mode)['--workspace-border'], palettes[mode].accent)
    assert.equal(appearanceVariables(mode, null, 100)['--workspace-border'], palettes[mode].accent)
    assert.equal(appearanceVariables(mode, '#AABBCC', 100)['--workspace-border'], '#AABBCC')
    assert.equal(appearanceVariables(mode, '#AABBCC', 0)['--workspace-border'], palettes[mode].border)
    assert.equal(appearanceVariables(mode, '#AABBCC', 45)['--workspace-border'], `color-mix(in srgb, #AABBCC 45%, ${palettes[mode].border})`)
    assert.equal(appearanceVariables(mode, null, 45)['--workspace-border'], `color-mix(in srgb, ${palettes[mode].accent} 45%, ${palettes[mode].border})`)
    const { '--workspace-border': border, ...custom } = appearanceVariables(mode, '#AABBCC', 45)
    const { '--workspace-border': originalBorder, ...original } = appearanceVariables(mode)
    assert.deepEqual(custom, original)
  }
})

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
