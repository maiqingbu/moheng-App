/* @vitest-environment happy-dom */
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { backupRestoreRequests, restorableBackups } from '../../src/ui/flows/backupRestore'
import { productErrorMessage } from '../../src/ui/presentation/productLanguage'

const source = (path: string) => fs.readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '../../', path), 'utf8')
const entry = { backupName: 'b04-cmd.db', checksumSha256: 'a'.repeat(64), createdAt: '2026-10-17T01:00:00Z', revision: 15, sizeBytes: 2048, available: true }

describe('FX-22 从备份恢复', () => {
  it('Given 选中的备份 When 确认恢复 Then 先自动备份当前状态，再按备份时的版本号恢复', () => {
    const [safety, restore] = backupRestoreRequests(entry)
    expect(safety).toEqual({ type: 'backup_create', payload: { kind: 'pre_restore' } })
    expect(restore).toEqual({
      type: 'backup_restore',
      expectedRevision: 15,
      payload: { backupName: 'b04-cmd.db', checksumSha256: 'a'.repeat(64) },
    })
  })

  it('Given 不可读的备份 When 列出 Then 不提供恢复', () => {
    expect(restorableBackups([entry, { ...entry, backupName: 'broken.db', revision: null, available: false }]).map(item => item.backupName)).toEqual(['b04-cmd.db'])
  })

  it('Given 设置页 When 检查 Then 列出备份、二次确认并说明会覆盖之后的改动；错误为中文', () => {
    const page = source('src/ui/pages/SettingsPage.vue')
    expect(page).toContain('data-backup-list')
    expect(page).toContain('备份之后的所有改动都会被覆盖')
    expect(page).toContain('backupRestoreRequests(')
    expect(source('src/infrastructure/tauri/frontendPort.ts')).toContain("'query_backups'")
    expect(source('src/application/contracts.ts')).toContain("| 'backup_restore'")
    expect(productErrorMessage('project command: READ_POOL_BUSY: restore requires all read leases to drain')).toContain('稍后')
    expect(productErrorMessage('project command: restore integrity failed: QUICK_CHECK_FAILED')).toContain('校验未通过')
  })
})
