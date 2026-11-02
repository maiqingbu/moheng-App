/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assistantIntentLabel, changeTypeForIntent, setupChangeRequest, setupDraftPatch, setupPathLabel } from '../../src/ui/presentation/assistantIntent'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')

describe('FX-21 助手语义理解', () => {
  it('Given 后端给出的修改意图 When 发起修改 Then 按意图而不是关键词决定事实修订', () => {
    expect(changeTypeForIntent({ intent: 'RETCON', source: 'MODEL' })).toBe('RETCON')
    expect(changeTypeForIntent({ intent: 'CHANGE', source: 'KEYWORD' })).toBe('CHANGE')
    const page = source('src/ui/pages/ProjectAssistant.vue')
    expect(page).not.toMatch(/\/事实\|改成\|原来\|没有死\|其实\//)
    expect(page).toContain("store.classifyAssistantIntent(question.value, 'CHANGE')")
  })

  it('Given 助手的理解结果 When 显示 Then 用中文说明理解成什么、来自模型还是关键词', () => {
    expect(assistantIntentLabel({ intent: 'KNOWLEDGE', source: 'MODEL' })).toBe('理解为：人物知情情况（模型理解）')
    expect(assistantIntentLabel({ intent: 'REBUILD_FTS', source: 'KEYWORD' })).toBe('理解为：重建搜索索引（关键词匹配）')
    expect(assistantIntentLabel({ intent: 'UNKNOWN_X', source: 'MODEL' })).toBe('')
    expect(assistantIntentLabel(undefined)).toBe('')
  })

  it('Given 用户说“以后某人不要……” When 发起修改 Then 生成纠正草稿，用户确认后才记住', () => {
    expect(changeTypeForIntent({ intent: 'REMEMBER', source: 'MODEL' })).toBe('REMEMBER')
    expect(assistantIntentLabel({ intent: 'REMEMBER', source: 'KEYWORD' })).toBe('理解为：记住一条写作规则（关键词匹配）')
    const page = source('src/ui/pages/ProjectAssistant.vue')
    // The proposal step stops at a draft; only the confirm button commits.
    const propose = page.slice(page.indexOf('async function proposeChange'), page.indexOf('async function confirmRemember'))
    expect(propose).toContain("if (changeType === 'REMEMBER')")
    expect(propose).not.toContain('memory_correction_add')
    const confirm = page.slice(page.indexOf('async function confirmRemember'))
    expect(confirm).toContain("type: 'memory_correction_add'")
    expect(page).toContain('@click="confirmRemember"')
  })

  it('Given 修改故事底稿的话 When 助手起草 Then 逐项显示原来和改成，按负责人改过的值提交申请', () => {
    expect(setupPathLabel('antagonist.method')).toBe('反派 · 手段')
    expect(setupPathLabel('cast.1.speechStyle')).toBe('配角 2 · 说话方式')
    const base = { antagonist: { name: '彭三', method: '借据' }, cast: [{ speechStyle: '话少' }, { speechStyle: '俗语' }] }
    const patch = setupDraftPatch(base, [
      { path: 'antagonist.method', before: '借据', after: '收买账房' },
      { path: 'cast.1.speechStyle', before: '俗语', after: '句句带刺' },
    ])
    expect(patch).toEqual({ antagonist: { name: '彭三', method: '收买账房' }, cast: [{ speechStyle: '话少' }, { speechStyle: '句句带刺' }] })
    expect(base.antagonist.method).toBe('借据')
    const page = source('src/ui/pages/ProjectAssistant.vue')
    const propose = page.slice(page.indexOf('async function proposeChange'), page.indexOf('async function confirmRemember'))
    expect(propose).toContain('store.draftSetupChange(question.value)')
    expect(propose).not.toContain('change_request_apply')
    const submit = page.slice(page.indexOf('async function submitSetupDraft'))
    expect(submit).toContain('payload: setupChangeRequest(changeId, setupDraft.value, question.value.trim())')
    const request = setupChangeRequest('change:1', { targetAuthority: { artifactType: 'StoryBible', artifactId: 'bible:1' }, items: [{ path: 'antagonist.method', before: '借据', after: '收买账房' }], base: { antagonist: { method: '借据' } }, patch: {} }, '改反派手段')
    expect(request.requestedChange).toEqual({ targetAuthority: { artifactType: 'StoryBible', artifactId: 'bible:1' }, patch: { antagonist: { method: '收买账房' } }, description: '改反派手段', source: 'PROJECT_ASSISTANT' })
    expect(request.sourceRefs).toEqual(['bible:1'])
    expect(page).toContain('data-setup-draft')
    expect(source('src/infrastructure/tauri/frontendPort.ts')).toContain("'draft_setup_change'")
  })

  it('Given 三种运行环境 When 检查接线 Then 桌面端调用后端命令、演示端有关键词兜底', () => {
    expect(source('src/infrastructure/tauri/frontendPort.ts')).toContain("'classify_assistant_intent'")
    expect(source('src/infrastructure/browser-demo/demoPort.ts')).toContain('async classifyAssistantIntent(')
    expect(source('src/application/contracts.ts')).toContain('classifyAssistantIntent(')
  })
})
