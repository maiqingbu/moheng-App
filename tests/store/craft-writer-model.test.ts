/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')

describe('写正文专用模型', () => {
  it('Given 设置页 When 检查 Then 可选填写写正文模型，留空沿用默认模型，并随设置保存', () => {
    const page = source('src/ui/pages/SettingsPage.vue')
    expect(page).toContain('data-render-model')
    expect(page).toContain('v-model="form.renderModelId"')
    expect(page).toContain('写正文模型（可选）')
    expect(page).toContain('留空则沿用默认创作模型')
    expect(page).toContain('renderModelId:(form.renderModelId ?? \'\').trim()')
    expect(source('src/application/contracts.ts')).toContain('renderModelId?: string')
    expect(page).toContain('data-audit-model')
    expect(page).toContain('v-model="form.auditModelId"')
    expect(page).toContain('审核模型（可选）')
    expect(page).toContain("auditModelId:(form.auditModelId ?? '').trim()")
    expect(source('src/application/contracts.ts')).toContain('auditModelId?: string')
  })
})

describe('P4 模型列表与思考开关', () => {
  it('Given 设置页 When 获取模型列表 Then 三个模型框都能从列表里选，也能手填', () => {
    const page = source('src/ui/pages/SettingsPage.vue')
    expect(page).toContain('@click="loadModels"')
    expect(page).toContain('<datalist id="provider-models">')
    expect((page.match(/list="provider-models"/g) ?? []).length).toBe(3)
    expect(source('src/infrastructure/tauri/frontendPort.ts')).toContain("'provider_list_models'")
    expect(source('src/infrastructure/browser-demo/demoPort.ts')).toContain('async listProviderModels(')
  })

  it('Given 三个步骤 When 设置思考 Then 每步可开关思考、选强度，并随设置保存', () => {
    const page = source('src/ui/pages/SettingsPage.vue')
    expect(page).toContain("{step:'default',label:'默认创作模型'},{step:'render',label:'写正文模型'},{step:'audit',label:'审核模型'}")
    expect(page).toContain('v-model="thinking[item.step].mode"')
    expect(page).toContain('v-model="thinking[item.step].effort"')
    expect(page).toContain('thinking:thinkingSettings()')
    // 服务商默认: nothing is saved for the step, so nothing is sent.
    expect(page).toContain('<option value="">服务商默认</option>')
    expect(source('src/application/contracts.ts')).toContain('thinking?: Partial<Record<ThinkingStep, StepThinking>>')
  })
})
