// FX-21: how the project assistant understood a question (backend intent).
import type { AssistantIntentResolution, SetupChangeDraft, SetupDraftItem } from '../../application/contracts'

const intentLabels: Record<string, string> = {
  PLANNING_CONSTRAINTS: '规划与任务约束',
  KNOWLEDGE: '人物知情情况',
  CONTINUITY: '承诺与伏笔',
  PROJECT_OVERVIEW: '作品整体状态',
  RETCON: '修订已有事实',
  REMEMBER: '记住一条写作规则',
  CHANGE: '一般修改',
  PROPOSAL_ONLY: '需要走修改提案',
  RECOMPILE_CHAPTER_PLAN: '重新生成本章计划',
  REPLAN_VOLUME_ROUTE: '重新规划本卷路线',
  REBUILD_FTS: '重建搜索索引',
  REBUILD_PROJECTION: '刷新页面显示数据',
  NO_ACTION: '暂无可执行的修复',
}

export function assistantIntentLabel(resolution: Pick<AssistantIntentResolution, 'intent' | 'source'> | null | undefined): string {
  const label = resolution ? intentLabels[resolution.intent] : undefined
  if (!resolution || !label) return ''
  return `理解为：${label}（${resolution.source === 'MODEL' ? '模型理解' : '关键词匹配'}）`
}

export function changeTypeForIntent(resolution: Pick<AssistantIntentResolution, 'intent' | 'source'>): 'RETCON' | 'REMEMBER' | 'CHANGE' {
  // P3: a lasting rule becomes an owner correction draft, never a change.
  if (resolution.intent === 'RETCON' || resolution.intent === 'REMEMBER') return resolution.intent
  return 'CHANGE'
}

// P3: the story bible fields a draft names, in the owner's words.
const setupFieldLabels: Record<string, string> = {
  angle: '角度', normalWorld: '日常世界', mechanism: '核心机制', protagonist: '主角', emotionalAnchor: '情感锚点',
  antagonist: '反派', cast: '配角', openingArc: '开篇走向', name: '名字', personality: '性格', flaw: '缺点',
  wantsNow: '眼下想要的', humorStyle: '幽默方式', innerVoiceStyle: '内心独白方式', position: '身份', wants: '想要的',
  method: '手段', role: '角色', relation: '关系', privateAgenda: '私心', speechStyle: '说话方式',
  contrastWithProtagonist: '和主角的反差', firstAppears: '首次出场章', experience: '体验', rules: '规则',
  rule: '规则', numbers: '数值', howReadersSeeIt: '读者怎么看到', object: '物件', memorySummary: '记忆',
  direction: '走向', protagonistWin: '主角的胜利', chapter: '章',
}

export function setupPathLabel(path: string): string {
  // An array index numbers the item before it: cast.1 is 配角 2.
  let label = ''
  for (const part of path.split('.')) {
    if (/^\d+$/.test(part)) label += ` ${Number(part) + 1}`
    else label += `${label ? ' · ' : ''}${setupFieldLabels[part] ?? part}`
  }
  return label
}

/** P3: the patch for a draft: its touched fields as they are, with each item's value set. */
export function setupDraftPatch(base: Record<string, unknown>, items: SetupDraftItem[]): Record<string, unknown> {
  const patch = structuredClone(base) as Record<string, unknown>
  for (const item of items) {
    const parts = item.path.split('.')
    let node = patch as Record<string, unknown>
    for (const part of parts.slice(0, -1)) node = node[part] as Record<string, unknown>
    node[parts[parts.length - 1]] = item.after
  }
  return patch
}

/** P3: the change request a reviewed draft becomes: its target and the patch. */
export function setupChangeRequest(changeRequestId: string, draft: SetupChangeDraft, words: string) {
  return {
    changeRequestId,
    changeType: 'CHANGE',
    scope: 'PROJECT',
    summary: words,
    reason: '用户从项目助手修改故事底稿',
    sourceRefs: [draft.targetAuthority.artifactId],
    requestedChange: {
      targetAuthority: draft.targetAuthority,
      patch: setupDraftPatch(draft.base, draft.items),
      description: words,
      source: 'PROJECT_ASSISTANT',
    },
  }
}
