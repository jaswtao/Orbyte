/** 导出折行与大小边界。Author: jaswtao · Date: 2026-10-14 */
import assert from 'node:assert/strict'
import test from 'node:test'
import { loadService } from './load-service.mjs'
const { exportBuffer, outputFilename } = await loadService('terminal/export')
const buffer = (lines) => ({ length: lines.length, getLine: (i) => lines[i] && ({ isWrapped: lines[i][1], translateToString: () => lines[i][0] }) })
test('还原折行和中文，使用 CRLF', () => assert.equal(exportBuffer(buffer([['中文', false], ['续行', true], ['下一行', false]])), '中文续行\r\n下一行'))
test('输出一万行不丢失', () => { const text = exportBuffer(buffer(Array.from({ length: 10000 }, (_, i) => [`行${i}`, false]))); assert.equal(text.split('\r\n').length, 10000); assert.ok(text.endsWith('行9999')) })
test('超限保留最新输出并注明', () => { const text = exportBuffer(buffer([['旧'.repeat(100), false], ['new', false]]), 110); assert.ok(text.startsWith('[Output exceeded')); assert.ok(text.endsWith('new')); assert.ok(new TextEncoder().encode(text).length <= 110) })
test('文件名替换 Windows 非法字符', () => assert.equal(outputFilename('项目:一', 'tab/?', new Date(2026, 9, 14, 12, 3, 4)), '项目_一-tab__-20261014-120304.txt'))
