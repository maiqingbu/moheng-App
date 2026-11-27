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
  // 2026-11-27 (owner approved the redesign): five task areas, decisions in the top bar.
  const areas = read('src/ui/presentation/workspaceAreas.ts')
  assert.match(shell, /WORKSPACE_AREAS/)
  for (const label of ['首页', '写作', '故事', '稿件', '资料库']) assert.match(areas, new RegExp(`label: '${label}', icon`))
  assert.match(shell, /件事等你决定/)
  // No button may promise what it does not do (the old search only opened the library).
  assert.doesNotMatch(shell, /⌘K 搜索/)
  assert.doesNotMatch(shell, /label:\s*['"]项目助手['"]|<span>项目助手<\/span>/)
  assert.match(dock, /supportSession/)
  assert.match(dock, /presentAssistantSession/)
  assert.match(dock, /确认|正式修改|查询/) 
  for (const marker of ['prototype-pagehead', 'prototype-grid3', 'prototype-grid', '下一步', '待决定', '作品状况']) assert.match(overview, new RegExp(marker))
})

test('Post-B14 shell does not introduce a frontend authority or browser fallback', () => {
  const shell = read('src/ui/components/AppShell.vue')
  const dock = read('src/ui/components/ProjectAssistantDock.vue')
  assert.doesNotMatch(`${shell}\n${dock}`, /localStorage|sessionStorage|indexedDB|rusqlite|libsql|invoke\(/)
  assert.doesNotMatch(`${shell}\n${dock}`, /BrowserDemo|浏览器演示适配器只用于 UI 验证/)
})

test('legacy assistant route redirects to the persistent dock instead of rendering a second page', () => {
  const router = read('src/app/router.ts')
  assert.match(router, /path:\s*['"]\/projects\/:id\/assistant['"][\s\S]*redirect:\s*to\s*=>/)
  assert.doesNotMatch(router, /path:\s*['"]\/projects\/:id\/assistant['"][\s\S]*component:\s*ProjectAssistant/)
})
