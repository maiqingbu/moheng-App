import type { BlockReason, ProjectPhase, RunState, SigningStatus } from '../../application/contracts'

export const phaseLabels: Record<ProjectPhase, string> = {
  SETUP: '项目准备中',
  PITCHING: '故事方向待确认',
  STORY_BLUEPRINT: '故事蓝图构建中',
  NAMING: '命名体系设计中',
  CONSTITUTION: '故事规则确认中',
  BOOK_PLANNING: '全书战略规划中',
  CANON_INITIALIZATION: '作品图谱初始化',
  OPENING_CONTRACT: '开篇规则编译中',
  OPENING_PRODUCTION: '前三章生产中',
  SIGNING: '开篇评估中',
  POST_SIGNING_PLANNING: '开篇已通过 · 规划长篇',
  VOLUME_PRODUCTION: '长篇生产中',
  ENDGAME: '终局收束中',
  COMPLETED: '作品已完成',
  EXPORTED: '已导出'
}

export const runStateLabels: Record<RunState, string> = {
  IDLE: '未运行',
  ACTIVE: '生产中',
  PAUSED: '已暂停',
  WAITING_DECISION: '等待决定',
  BLOCKED: '已阻断',
  FAILED: '本次失败',
  COMPLETED: '已完成'
}

export const signingLabels: Record<SigningStatus, string> = {
  NOT_STARTED: '未评估',
  IN_REVIEW: '评估中',
  REVISE: '未通过（历史评估）',
  REJECTED: '未通过',
  PASS: '已通过',
  FAIL: '评估未通过',
  NOT_VERIFIED: '无法确认',
  STALE: '需要重新评估'
}

export const blockReasonLabels: Record<Exclude<BlockReason, null>, string> = {
  USER_PAUSE: '用户主动暂停',
  BUDGET_LIMIT: '预算达到上限',
  DECISION_REQUIRED: '等待你的决定',
  POLICY_EXPIRED: '平台政策已过期',
  SIGNING_REQUIRED: '开篇评估未通过',
  SIGNING_INVALIDATED: '开篇评估已失效',
  CONTRACT_FAILURE: '创作规则检查未通过',
  CANON_CONFLICT: '正式设定发生冲突',
  RETRY_EXHAUSTED: '自动修复次数已用尽',
  PERSISTENCE_FAILURE: '本机数据写入失败',
  UNKNOWN_REMOTE_OUTCOME: '远端请求结果未知，等待处理决定'
}

export function phaseLabel(phase?: ProjectPhase) {
  return phase ? phaseLabels[phase] : '项目准备中'
}

export function runStateLabel(state?: RunState) {
  return state ? runStateLabels[state] : '未运行'
}
