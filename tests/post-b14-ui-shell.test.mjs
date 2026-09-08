import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8')

test('Post-B14 shell uses task-oriented IA and a real assistant dock', () => {
  const shell = read('src/ui/components/AppShell.vue')
  const dock = read('src/ui/components/ProjectAssistantDock.vue')
  const overview = read('src/ui/pages/ProjectOverview.vue')
  assert.match(shell, /ProjectAssistantDock/)
  for (const label of ['总览', '创作', '规划', '当前卷', '待处理', '长篇健康', '资料', '完结准备']) assert.match(shell, new RegExp(label))
  assert.doesNotMatch(shell, /label:\s*['"]项目助手['"]|<span>项目助手<\/span>/)
  assert.match(dock, /supportSession/)
  assert.match(dock, /presentAssistantSession/)
  assert.match(dock, /确认|正式修改|查询/) 
  for (const marker of ['prototype-pagehead', 'prototype-grid3', 'prototype-grid', '继续创作', '需要处理', '长篇状态']) assert.match(overview, new RegExp(marker))
})

test('Post-B14 shell does not introduce a frontend authority or browser fallback', () => {
  const shell = read('src/ui/components/AppShell.vue')
  const dock = read('src/ui/components/ProjectAssistantDock.vue')
  assert.doesNotMatch(`${shell}\n${dock}`, /localStorage|sessionStorage|indexedDB|rusqlite|libsql|invoke\(/)
  assert.doesNotMatch(`${shell}\n${dock}`, /BrowserDemo|浏览器演示适配器只用于 UI 验证/)
})
