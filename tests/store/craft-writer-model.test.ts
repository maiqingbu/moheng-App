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
