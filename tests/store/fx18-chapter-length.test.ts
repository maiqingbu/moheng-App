/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import { createApp, nextTick, ref, h } from 'vue'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { CHAPTER_LENGTH_PRESETS, DEFAULT_CHAPTER_LENGTH_RANGE, PLATFORM_CHAPTER_TARGETS, chapterLengthLabel, isValidChapterLengthRange, withChapterLengthRange } from '../../src/ui/presentation/chapterLength'

const page = (name: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), `../../src/ui/pages/${name}.vue`), 'utf8')
import { productErrorMessage } from '../../src/ui/presentation/productLanguage'

describe('FX-18 章节字数区间', () => {
  it('Given 新建作品的平台下拉 When 对照后端平台字数表 Then 每个平台的目标字数一致', () => {
    const rust = fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../src-tauri/src/chapter_length.rs'), 'utf8')
    for (const { name, target } of PLATFORM_CHAPTER_TARGETS) {
      const row = rust.split('\n').find(line => line.includes(`"${name.slice(0, 2)}"`) && line.includes('_000'))
      expect(row, name).toBeDefined()
      expect(Number(row!.match(/(\d)_(\d{3})/)!.slice(1).join('')), name).toBe(target)
    }
  })

  it('Given 预设与自定义区间 When 校验 Then 与后端规则一致', () => {
    expect(DEFAULT_CHAPTER_LENGTH_RANGE).toEqual({ min: 2000, max: 3000 })
    expect(CHAPTER_LENGTH_PRESETS.map(chapterLengthLabel)).toEqual(['2000–3000 字', '3000–4000 字', '4000–5000 字'])
    expect(isValidChapterLengthRange({ min: 2500, max: 3500 })).toBe(true)
    expect(isValidChapterLengthRange({ min: 3000, max: 3000 })).toBe(false)
    expect(isValidChapterLengthRange({ min: 400, max: 1000 })).toBe(false)
    expect(isValidChapterLengthRange({ min: 2000, max: 12000 })).toBe(false)
    expect(isValidChapterLengthRange({ min: 2000.5, max: 3000 })).toBe(false)
  })

  it('Given 字数相关错误 When 显示 Then 是中文说明且不暴露编码', () => {
    expect(productErrorMessage('CHAPTER_LENGTH_RANGE_INVALID')).toContain('字数区间')
    expect(productErrorMessage('B08_PLAN_LENGTH_OUT_OF_RANGE')).toContain('规划')
    const render = productErrorMessage('B08_RENDER_LENGTH_OUT_OF_RANGE:600:2000-3000')
    expect(render).toContain('字数')
    expect(render).not.toContain('B08')
  })

  it('Given 单章选择器 When 选“作品默认”、预设和自定义 Then 依次给出 null、预设区间和自定义区间', async () => {
    const { default: ChapterLengthPicker } = await import('../../src/ui/components/ChapterLengthPicker.vue')
    const value = ref<{ min: number; max: number } | null>(null)
    const host = document.createElement('div')
    const app = createApp({ render: () => h(ChapterLengthPicker, { modelValue: value.value, allowDefault: true, defaultRange: { min: 3000, max: 4000 }, 'onUpdate:modelValue': (next: { min: number; max: number } | null) => { value.value = next } }) })
    app.mount(host)
    await nextTick()
    const select = host.querySelector('select') as HTMLSelectElement
    expect(select.options[0].textContent).toContain('作品默认（3000–4000 字）')
    select.value = 'preset:1'; select.dispatchEvent(new Event('change')); await nextTick()
    expect(value.value).toEqual({ min: 3000, max: 4000 })
    select.value = 'custom'; select.dispatchEvent(new Event('change')); await nextTick()
    const [min, max] = Array.from(host.querySelectorAll('input')) as HTMLInputElement[]
    min.value = '2600'; min.dispatchEvent(new Event('input'))
    max.value = '3600'; max.dispatchEvent(new Event('input')); await nextTick()
    expect(value.value).toEqual({ min: 2600, max: 3600 })
    max.value = '2000'; max.dispatchEvent(new Event('input')); await nextTick()
    expect(host.textContent).toContain('最少 500 字')
    expect(value.value).toEqual({ min: 2600, max: 3600 })
    select.value = 'default'; select.dispatchEvent(new Event('change')); await nextTick()
    expect(value.value).toBeNull()
    app.unmount()
  })

  it('Given 单章区间 When 组装命令 Then 只有有效区间才写入 payload', () => {
    expect(withChapterLengthRange({ chapterId: 'c' }, { min: 3000, max: 4000 })).toEqual({ chapterId: 'c', chapterLengthRange: { min: 3000, max: 4000 } })
    expect(withChapterLengthRange({ chapterId: 'c' }, null)).toEqual({ chapterId: 'c' })
    expect(withChapterLengthRange({ chapterId: 'c' }, { min: 3000, max: 2000 })).toEqual({ chapterId: 'c' })
  })

  it('Given 三个入口 When 检查页面 Then 作品设置、开篇章节开始生产、长篇章节生成计划都可选字数区间', () => {
    const settings = page('SettingsPage')
    expect(settings).toContain('data-chapter-length-setting')
    expect(settings).toContain('<ChapterLengthPicker v-model="form.chapterLengthRange" />')
    const production = page('ProductionCenter')
    expect(production).toContain('data-chapter-length-override')
    expect(production).toContain("chapter.executionMode === 'LONG_FORM' ? null : chapterLengthRange.value")
    const volume = page('VolumeWorkspace')
    expect(volume).toContain('}, chapterLengthRange.value),')
    expect(volume).toContain('章字数区间（生成计划时生效）')
  })
})
