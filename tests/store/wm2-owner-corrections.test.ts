/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')

describe('负责人纠正（WM-2a）', () => {
  it('Given 计划提醒或连贯提醒 When 打开 Then 可以记住这条，改好措辞、可选填对谁生效后保存为负责人纠正', () => {
    const inbox = source('src/ui/pages/DecisionInbox.vue')
    expect(inbox).toContain('v-if="selected.rememberable && !remembering"')
    expect(inbox).toContain('记住这条')
    expect(inbox).toContain("type: 'memory_correction_add'")
    expect(inbox).toContain('decisionId: selected.value.id, content: correction.value, subject: subject.value')
    expect(source('src-tauri/src/repositories/ui_projection.rs')).toContain('Some("plan" | "continuity")')
  })

  it('Given 作品设置 When 打开写作规矩 Then 列出现有规矩，可以新增和撤销', () => {
    const page = source('src/ui/pages/SettingsPage.vue')
    expect(page).toContain('id="setting-rules"')
    expect(page).toContain("type:'writing_rule_add'")
    expect(page).toContain("type:'memory_correction_revoke'")
    expect(page).toContain('projectStore.loadCorrections()')
    const contracts = source('src/application/contracts.ts')
    expect(contracts).toContain("| 'writing_rule_add'")
    expect(contracts).toContain('queryOwnerCorrections(request: ProjectQueryRequest)')
  })
})
