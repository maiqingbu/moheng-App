import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { PROVIDER_PRESETS, modelChoices, presetForEndpoint } from '../../src/ui/presentation/providerPresets'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')

describe('内置服务商预设', () => {
  it('Given 常用服务商 When 列出预设 Then 每个都有中文名和兼容接口地址，最后一项是自定义', () => {
    const named = PROVIDER_PRESETS.filter(preset => preset.id !== 'custom')
    expect(named.map(preset => preset.label)).toEqual(['阿里云百炼', 'DeepSeek', 'Kimi（月之暗面）', '智谱', '火山方舟（豆包）', '硅基流动', 'OpenAI'])
    for (const preset of named) expect(preset.endpoint).toMatch(/^https:\/\/[^\s]+$/)
    expect(PROVIDER_PRESETS.at(-1)).toMatchObject({ id: 'custom', endpoint: '' })
  })

  it('Given 已保存的接口地址 When 打开设置 Then 认出它属于哪家服务商，百炼的专属工作空间地址也算百炼', () => {
    expect(presetForEndpoint('https://ws-a4ht0p7fdc83x4x4.cn-beijing.maas.aliyuncs.com/compatible-mode/v1').id).toBe('bailian')
    expect(presetForEndpoint('https://api.deepseek.com/v1').id).toBe('deepseek')
    expect(presetForEndpoint('https://example.com/v1').id).toBe('custom')
    expect(presetForEndpoint('').id).toBe('custom')
  })

  it('Given 预设的常用模型和接口读到的模型 When 合成下拉选项 Then 常用模型在前，不重复', () => {
    const bailian = PROVIDER_PRESETS.find(preset => preset.id === 'bailian')!
    const choices = modelChoices(bailian, ['qwen-plus', 'qwen-long'])
    expect(choices.slice(0, bailian.models.length)).toEqual(bailian.models)
    expect(choices).toContain('qwen-long')
    expect(new Set(choices).size).toBe(choices.length)
  })

  it('Given 设置页 When 选服务商 Then 自动填接口地址并读取模型列表，三个模型框都用同一份下拉', () => {
    const page = source('src/ui/pages/SettingsPage.vue')
    expect(page).toContain('data-provider-preset')
    expect(page).toContain('@change="choosePreset"')
    expect(page).toContain('modelChoices(preset,models)')
    expect((page.match(/list="provider-models"/g) ?? []).length).toBe(3)
  })
})
