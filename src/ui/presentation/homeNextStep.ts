import type { ProjectSnapshot } from '../../application/contracts'
import { blockReasonLabels } from './projectLabels'

export interface HomeNextStep {
  title: string
  note: string
  action: string
  /** The path under /projects/:id. */
  path: string
}

/** 2026-11-27: the one next step the home page offers, said plainly, from the
 *  work's own state; its button goes exactly where that step is done. */
export function homeNextStep(project: Pick<ProjectSnapshot, 'runState' | 'phase' | 'blockReason'>, pending: number, latestChapter: number | null): HomeNextStep {
  if (pending) return { title: `先处理 ${pending} 件待决定`, note: '它们会影响接下来怎么写，处理完就能继续。', action: '去处理', path: '/decisions' }
  switch (project.runState) {
    case 'ACTIVE': return { title: '正在写作中', note: '写完、检查完会自动入库，可以随时去看进度。', action: '查看写作进度', path: '/production' }
    case 'BLOCKED':
    case 'FAILED': return { title: '写作停下来了', note: project.blockReason ? `原因：${blockReasonLabels[project.blockReason]}。` : '上一次没有写完，需要看一下原因。', action: '去看原因', path: '/production' }
    case 'PAUSED': return { title: '写作已暂停', note: '准备好了就从写作页继续。', action: '去写作页', path: '/production' }
  }
  if (project.phase === 'PITCHING' || project.phase === 'STORY_BLUEPRINT') return { title: '先定好故事方向', note: '方向和设定确认后，就能开始写第一章。', action: '去定方向', path: '/directions' }
  return latestChapter
    ? { title: `写第 ${latestChapter + 1} 章`, note: `第 ${latestChapter} 章已经入库。`, action: '去写作', path: '/production' }
    : { title: '写第 1 章', note: '设定已经就绪，可以开始写第一章。', action: '去写作', path: '/production' }
}
