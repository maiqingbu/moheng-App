export type ProductReceiptSummary = {
  status: 'success' | 'failed' | 'pending'
  title: string
  detail: string
}

const statusLabels: Record<string, string> = {
  idle: '',
  submitting: '正在处理',
  accepted: '已接收',
  processing: '处理中',
  committed: '已完成',
  blocked: '暂时无法继续',
  failed: '未完成',
  ACTIVE: '进行中',
  RUNNING: '进行中',
  QUEUED: '等待处理',
  PAUSED: '已暂停',
  WAITING_DECISION: '等待你的决定',
  WAITING_MODEL: '等待模型服务响应',
  WAITING_BUDGET: '等待预算决定',
  COMPLETED: '已完成',
  CANCELLED: '已取消',
  FAILED: '未完成',
  BLOCKED: '暂时无法继续',
  done: '已完成',
  active: '进行中',
  locked: '待解锁',
  impact_ready: '影响范围已算出',
  draft: '草稿',
  pending: '待确认',
  OPEN: '进行中',
  SUSPENDED: '暂缓',
  RESOLVED: '已解决',
  KNOWS: '已知',
  BELIEVES: '相信',
  BELIEVES_TRUE: '相信',
  BELIEVES_FALSE: '不再相信',
  CONCEALS: '刻意隐瞒',
  UNKNOWN: '未知',
  PROPOSED: '待确认',
  AUTHORIZED: '已确认，等待执行',
  EXECUTABLE: '可以执行',
  PROPOSAL_ONLY: '需要先建立修改方案',
  NO_ACTION: '无需处理',
  VERIFIED: '已完成并核对',
  REPAIR_VERIFIED: '修复已完成并核对',
  INCOMPLETE: '尚未完成',
  PASS: '已通过',
  VALID: '有效',
  STALE: '需要重新确认',
  PLAN_STALE: '当前规划已与最新内容不一致，需要重新确认或更新',
  AUTHORIZATION_STALE: '作品状态已经变化，请重新确认修复方案',
  EVIDENCE_GAP: '现有信息不足，暂时无法给出可靠结论',
  IPC_UNSUPPORTED: '当前功能暂不可用，请返回上一页稍后再试',
  UNKNOWN_REMOTE_OUTCOME: '远程结果尚未确认，需要你的决定',
  PENDING_APPROVAL: '等待确认',
  IMPACT_READY: '影响范围已算出',
  DRAFT: '草稿',
  APPROVED: '已确认',
  REJECTED: '已拒绝',
}

const errorLabels: Record<string, string> = {
  PLAN_STALE: '当前规划已与最新内容不一致，需要重新确认或更新。',
  AUTHORIZATION_STALE: '作品状态已经变化，请重新确认修复方案。',
  EVIDENCE_GAP: '现有信息不足，暂时无法给出可靠结论。请补充对应章节或设定依据。',
  IPC_UNSUPPORTED: '当前功能暂不可用，请返回上一页稍后再试。',
  'PROTOCOL / PROJECT_BINDING_ERROR': '作品状态已经变化，请刷新当前作品后重试。',
  'PROTOCOL / PROJECT_REVISION_UNAVAILABLE': '暂时无法确认作品的最新状态，请刷新当前作品后重试。',
  'PROTOCOL / RESULT_PROJECT_ID_MISSING': '副本创建未完成，请刷新作品库后重试。',
  B12_RETCON_EVIDENCE_GAP: '这条事实目前缺少可核验的来源记录，不能直接执行事实修订。',
  B12_RETCON_EVIDENCE_UNRELATED: '所选依据与这条事实没有明确关系，请重新选择依据。',
  B12_RETCON_FACT_SOURCE_REQUIRED: '请先选择这条事实的原始依据，再继续制定修订方案。',
  B12_RETCON_EVIDENCE_NOT_FOUND: '所选依据已不可用，请重新查询后选择。',
  B12_CANON_FACT_NOT_FOUND: '没有找到要修订的正式事实，请刷新后重新选择。',
  B12_USER_DECISION_REQUIRED: '这项操作需要你的明确确认。',
  B12_CHANGE_REQUEST_NOT_FOUND: '没有找到这项修改请求，请刷新记录后重试。',
  'B12_DECISION_STALE': '作品状态已经变化，原确认已失效，请重新查看方案。',
  'B12_RETCON_EVIDENCE_REQUIRED': '请先提供这条事实对应的核验依据。',
  'PROTOCOL / PROJECT_NOT_FOUND': '没有找到这部作品，请返回作品库重新打开。',
}

function textOf(value: unknown): string {
  if (value instanceof Error) return value.message
  return typeof value === 'string' ? value : ''
}

export function isProductUxEvidenceDemoEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_DEMO === 'true'
}

const internalProductTokenPattern = /(?:\b(?:Rust|TypeScript|JavaScript|Vue|Pinia|Tauri|IPC|SQLite|SQL|WebView|Keychain|Runtime|Owner|Repository|Artifact|Harness|CommitBundle|CommitService|Narrative Authority|PlanningOwner|ProjectionOwner|VolumeLifecycleOwner|RuntimeCheckRequest|RepairPlan|CandidatePatch|RepairSuggestion)\b|\b(?:projectId|artifactId|payloadHash|changeRequestId|authorizationId|repairPlanId|operationDigest|expectedRevision|sourceCommit|ownerReceiptRef|stateDigest|sourceRefs|artifactRef|capabilityId|skillId)\b|\bSkill\s+ID\b|\bskill[:_-][A-Za-z0-9_-]+\b|(?:PLAN_STALE|EVIDENCE_GAP|AUTHORIZATION_STALE|IPC_UNSUPPORTED|B12_[A-Z0-9_]+)\b|PROTOCOL\s*\/|\bP[0-3]\b|(?:B\d{2}|RC\d+)::[A-Za-z0-9_:.-]+|(?:^|[\s(])(?:\/|[A-Za-z]:[\\/]|(?:src|tests|scripts|src-tauri)[\\/])\S+|[{}\[\]]|\b(?:SELECT|INSERT|UPDATE|DELETE|FROM|WHERE|JOIN|PRAGMA)\b)/i
const capabilityPairPattern = /\bcapability\s+(?:query|diagnose)_[a-z0-9_]+\b/i

export function containsInternalProductToken(value: string): boolean {
  return internalProductTokenPattern.test(value) || capabilityPairPattern.test(value)
}

function looksInternal(value: string): boolean {
  return containsInternalProductToken(value)
}

const providerConnectionPattern = /(?:MODEL|ENDPOINT|API[_ ]KEY|TRANSPORT|连接服务|模型服务)/i
const implementationTokenPattern = /\b(?:Rust|TypeScript|JavaScript|Vue|SQLite|WebView|IPC)\b/i

export function statusLabel(value: unknown, fallback = '需要继续核对'): string {
  const raw = textOf(value).trim()
  if (!raw) return fallback
  if (statusLabels[raw]) return statusLabels[raw]
  const upper = raw.toUpperCase()
  if (statusLabels[upper]) return statusLabels[upper]
  return /[\u4e00-\u9fff]/.test(raw) && !looksInternal(raw) ? raw : fallback
}

export function scopeLabel(value: unknown): string {
  return ({ GLOBAL: '全书范围', WIDE: '较大范围', LOCAL: '局部范围' } as Record<string, string>)[textOf(value).toUpperCase()] ?? '当前范围'
}

export function changeStatusLabel(value: unknown): string {
  return statusLabel(value, '待确认')
}

export function riskLabel(value: unknown): string {
  return ({ HIGH: '高风险', MEDIUM: '需要关注', LOW: '较低风险' } as Record<string, string>)[textOf(value).toUpperCase()] ?? '需要关注'
}

export function severityLabel(value: unknown): string {
  return ({ risk: '高影响', attention: '需要关注', info: '提示' } as Record<string, string>)[textOf(value).toLowerCase()] ?? '需要关注'
}

export function productErrorMessage(value: unknown, fallback = '操作未完成，请刷新当前作品后重试。'): string {
  const raw = textOf(value).trim()
  if (!raw) return fallback
  for (const [code, message] of Object.entries(errorLabels)) {
    if (raw === code || raw.includes(code)) return message
  }
  if (providerConnectionPattern.test(raw) && !implementationTokenPattern.test(raw)) {
    return '模型服务连接失败，请检查服务地址和访问密钥后重试。'
  }
  if (containsInternalProductToken(raw) || !/[\u4e00-\u9fff]/.test(raw) || /[{}\[\]]/.test(raw) || /\b(?:SELECT|INSERT|UPDATE|DELETE|FROM|WHERE)\b/i.test(raw)) {
    return fallback
  }
  return raw
}

export function sanitizeProductText(value: unknown, fallback = ''): string {
  const raw = textOf(value).trim()
  if (!raw) return fallback
  if (containsInternalProductToken(raw) || !/[\u4e00-\u9fff]/.test(raw) || /[{}\[\]]/.test(raw) || /\b(?:SELECT|INSERT|UPDATE|DELETE|FROM|WHERE)\b/i.test(raw)) {
    return fallback
  }
  return raw
}

export function evidenceLabel(value: unknown): string {
  return sanitizeProductText(value, '这条事实的原始依据')
}

export type ProductEvidenceIdentity = {
  artifactId: string
  version: number
}

export function evidenceSelector(item: ProductEvidenceIdentity): string {
  return `${item.artifactId}:${item.version}`
}

export function evidenceDisplayLabel(item: { label: unknown }, index: number): string {
  return `${evidenceLabel(item.label)}（${index + 1}）`
}

export function selectEvidenceBySelector<T extends ProductEvidenceIdentity>(items: readonly T[], selector: string): T | undefined {
  return items.find(item => evidenceSelector(item) === selector)
}

export type ProductEvidenceLookup = ProductEvidenceIdentity & { payloadHash?: string }

export function selectEvidenceByIdentity<T extends ProductEvidenceLookup>(items: readonly T[], identity: ProductEvidenceLookup): T | undefined {
  return items.find(item => item.artifactId === identity.artifactId
    && item.version === identity.version
    && (identity.payloadHash === undefined || item.payloadHash === identity.payloadHash))
}

export function commandMessage(state: { status?: unknown; message?: unknown } | unknown): string {
  const record = state && typeof state === 'object' ? state as Record<string, unknown> : {}
  const message = productErrorMessage(record.message, '')
  return message || statusLabel(record.status, '')
}

export function receiptSummary(receipt: unknown): ProductReceiptSummary {
  const record = receipt && typeof receipt === 'object' ? receipt as Record<string, unknown> : {}
  const finalStatus = textOf(record.finalStatus).toLowerCase()
  const result = record.result && typeof record.result === 'object' ? record.result as Record<string, unknown> : {}
  if (finalStatus === 'committed') {
    return {
      status: 'success',
      title: '已完成并核对',
      detail: '操作已经完成，结果已重新核对。',
    }
  }
  if (finalStatus === 'failed' || finalStatus === 'blocked') {
    const detail = productErrorMessage(record.message, statusLabel(result.status, '请先处理当前提示'))
    return {
      status: 'failed',
      title: finalStatus === 'blocked' ? '暂时无法继续' : '未完成',
      detail: `${detail.replace(/[。.]$/, '')}。`,
    }
  }
  return { status: 'pending', title: statusLabel(finalStatus, '处理中'), detail: '系统正在处理这项操作，请稍候。' }
}
