/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chapterQuotaLabel, chapterQuotaResetRequest, isChapterQuotaExhausted } from '../../src/ui/presentation/chapterQuota'
import { productErrorMessage } from '../../src/ui/presentation/productLanguage'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')

describe('手动重置本章额度', () => {
  it('Given 本章额度 When 判断 Then 用满才可重置，并用中文说明用量', () => {
    expect(isChapterQuotaExhausted({ used: 12, limit: 12, allowance: 12 })).toBe(true)
    expect(isChapterQuotaExhausted({ used: 13, limit: 12, allowance: 12 })).toBe(true)
    expect(isChapterQuotaExhausted({ used: 5, limit: 12, allowance: 12 })).toBe(false)
    expect(isChapterQuotaExhausted(null)).toBe(false)
    expect(chapterQuotaLabel({ used: 12, limit: 12, allowance: 12 })).toBe('本章已用模型调用 12 / 12 次')
  })

  it('Given 用户确认重置 When 组装命令 Then 只带章节编号', () => {
    expect(chapterQuotaResetRequest('chapter:2')).toEqual({ type: 'production_reset_chapter_quota', payload: { chapterId: 'chapter:2' } })
  })

  it('Given 创作页 When 检查 Then 额度用满时显示二次确认的重置按钮；额度错误为中文并提示可重置', () => {
    const page = source('src/ui/pages/ProductionCenter.vue')
    expect(page).toContain('data-chapter-quota')
    expect(page).toContain('重置本章额度')
    expect(page).toContain('确认重置')
    expect(page).toContain('{{ chapterQuota.allowance }} 次')
    expect(page).toContain('chapterQuotaResetRequest(')
    expect(source('src/application/contracts.ts')).toContain("| 'production_reset_chapter_quota'")
    const message = productErrorMessage('BUDGET_EXCEEDED')
    expect(message).toContain('重置本章额度')
    expect(message).not.toContain('BUDGET')
  })
})
