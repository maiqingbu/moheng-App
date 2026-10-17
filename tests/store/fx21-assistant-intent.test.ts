/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assistantIntentLabel, changeTypeForIntent } from '../../src/ui/presentation/assistantIntent'

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

  it('Given 三种运行环境 When 检查接线 Then 桌面端调用后端命令、演示端有关键词兜底', () => {
    expect(source('src/infrastructure/tauri/frontendPort.ts')).toContain("'classify_assistant_intent'")
    expect(source('src/infrastructure/browser-demo/demoPort.ts')).toContain('async classifyAssistantIntent(')
    expect(source('src/application/contracts.ts')).toContain('classifyAssistantIntent(')
  })
})
