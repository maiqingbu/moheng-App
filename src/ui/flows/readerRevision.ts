import type { CommandRequest } from '../../application/contracts'

export const READER_REVISION_SOURCE = 'MANUSCRIPT_READER'

export function isReaderProposalOnlySource(source: unknown): boolean {
  return source === READER_REVISION_SOURCE
}

export function readerProposalStatusLabel(status: string): string {
  if (status === 'REJECTED') return '建议已拒绝'
  if (status === 'CANCELLED') return '影响分析已取消'
  return '建议已留存；正文未修改'
}

export function readerHistoryStatusLabel(entry: {
  source?: string | null
  status: string
  targetClass?: string | null
}): string | null {
  if (!isReaderProposalOnlySource(entry.source)) return null
  if (entry.status === 'rejected') return '建议已拒绝'
  if (entry.status !== 'applied') return '建议已留存；正文未修改'
  if (entry.targetClass === 'PROJECTION_ONLY') return '建议已处理；正文未修改'
  if (entry.targetClass === 'PLAN_CHANGE') return '已形成正式规划变更；正文状态请查看版本记录'
  return '建议已处理；正文状态待核对'
}

export interface ReaderRevisionInput {
  projectId: string
  chapterId: string
  chapterNumber: number
  chapterTitle: string
  feedback: string
}

export function buildReaderRevisionCommands(
  input: ReaderRevisionInput,
  now = Date.now(),
): { changeRequestId: string; proposal: CommandRequest; impact: CommandRequest } {
  const projectId = input.projectId.trim()
  const chapterId = input.chapterId.trim()
  const feedback = input.feedback.trim()
  if (!projectId || !chapterId || !feedback) {
    throw new Error('请先选择章节并填写修订建议。')
  }
  const changeRequestId = `change:${projectId}:reader:${now}`
  return {
    changeRequestId,
    proposal: {
      type: 'change_request_create',
      projectId,
      payload: {
        changeRequestId,
        changeType: 'CHANGE',
        scope: 'PROJECT',
        summary: `第${input.chapterNumber}章修改建议`,
        reason: '用户从正式阅读页发起章节修改请求。',
        targetRefs: [chapterId],
        sourceRefs: [chapterId],
        requestedChange: {
          description: feedback,
          source: READER_REVISION_SOURCE,
          targetChapter: {
            chapterId,
            chapterNumber: input.chapterNumber,
            title: input.chapterTitle,
          },
        },
      },
    },
    impact: {
      type: 'impact_preflight',
      projectId,
      payload: { changeRequestId },
    },
  }
}
