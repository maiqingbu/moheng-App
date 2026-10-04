import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { buildReaderRevisionCommands, isReaderProposalOnlySource, readerHistoryStatusLabel, readerProposalStatusLabel } from '../../src/ui/flows/readerRevision'

const root = resolve(import.meta.dirname, '../..')
const read = (relative: string) => readFileSync(resolve(root, relative), 'utf8')

describe('B2 Reader capability truth', () => {
  it('keeps Reader feedback proposal-only without exposing generic Change apply', () => {
    expect(isReaderProposalOnlySource('MANUSCRIPT_READER')).toBe(true)
    expect(isReaderProposalOnlySource('HISTORY_CENTER')).toBe(false)
    expect(readerProposalStatusLabel('APPLIED')).toBe('建议已留存；正文未修改')
    expect(readerProposalStatusLabel('REJECTED')).toBe('建议已拒绝')
    expect(readerHistoryStatusLabel({ source: 'MANUSCRIPT_READER', status: 'impact_ready' })).toBe('建议已留存；正文未修改')
    expect(readerHistoryStatusLabel({ source: 'MANUSCRIPT_READER', status: 'rejected' })).toBe('建议已拒绝')
    expect(readerHistoryStatusLabel({ source: 'MANUSCRIPT_READER', status: 'applied', targetClass: 'PROJECTION_ONLY' })).toBe('建议已处理；正文未修改')
    expect(readerHistoryStatusLabel({ source: 'MANUSCRIPT_READER', status: 'applied', targetClass: 'PLAN_CHANGE' })).toBe('已形成正式规划变更；正文状态请查看版本记录')
    expect(readerHistoryStatusLabel({ source: 'MANUSCRIPT_READER', status: 'applied' })).toBe('建议已处理；正文状态待核对')
    expect(readerHistoryStatusLabel({ source: 'HISTORY_CENTER', status: 'applied' })).toBeNull()

    const history = read('src/ui/pages/HistoryCenter.vue')
    expect(history).toMatch(/if\s*\(isReaderProposalOnlySource\(activeChange\.value\.source\)\)/)
    expect(history).toMatch(/v-if="!readerProposalOnly"[^>]*@click="approveAndApply"/)
    expect(history).toMatch(/章节修改建议仅作为意见留存/)
    expect(history).toMatch(/activeChangeStatusLabel/)
    expect(history).toMatch(/historyEntryStatusLabel\(entry\)/)
    expect(history).toMatch(/historyEntryStatusClass\(entry\)/)
  })

  it('creates a formal ChangeRequest and impact preflight for Reader feedback', () => {
    const commands = buildReaderRevisionCommands({
      projectId: 'project:one',
      chapterId: 'chap:one',
      chapterNumber: 7,
      chapterTitle: '雨夜',
      feedback: '  主角更主动  ',
    }, 123)
    expect(commands.changeRequestId).toBe('change:project:one:reader:123')
    expect(commands.proposal).toMatchObject({
      type: 'change_request_create',
      projectId: 'project:one',
      payload: {
        changeRequestId: commands.changeRequestId,
        changeType: 'CHANGE',
        scope: 'PROJECT',
        targetRefs: ['chap:one'],
        sourceRefs: ['chap:one'],
        requestedChange: {
          description: '主角更主动',
          source: 'MANUSCRIPT_READER',
          targetChapter: { chapterId: 'chap:one', chapterNumber: 7, title: '雨夜' },
        },
      },
    })
    expect(commands.impact).toEqual({
      type: 'impact_preflight',
      projectId: 'project:one',
      payload: { changeRequestId: commands.changeRequestId },
    })

    const reader = read('src/ui/pages/ManuscriptReader.vue')
    const history = read('src/ui/pages/HistoryCenter.vue')
    expect(reader).toMatch(/buildReaderRevisionCommands/)
    expect(reader).not.toMatch(/chapter_revision_request|type:\s*['"]export_create['"]|reader-export/)
    expect(reader).toMatch(/name:\s*['"]history['"]/)
    expect(history).toMatch(/queryChangeRequest/)
  })

  it('loads formal export capability and requires backend receipt for success', () => {
    const reader = read('src/ui/pages/ManuscriptReader.vue')
    expect(reader).toMatch(/loadExportCapability/)
    expect(reader).toMatch(/createExport/)
    expect(reader).toMatch(/queryExportReceipt/)
    expect(reader).toMatch(/exportReceipt(?:\.value)?\?\.status\s*===\s*['"]COMPLETED['"]/)
    expect(reader).toMatch(/exportProgress/)
    expect(reader).toMatch(/导出正式成品/)
  })
})
