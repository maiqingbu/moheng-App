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
  NOT_STARTED: '未开始',
  SATISFIED: '已满足',
  INVALIDATED: '已失效',
  ABANDONED: '已放弃',
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
  REPAIR_VERIFICATION_FAILED: '结果未通过核验',
  REPAIR_VERIFICATION_INCOMPLETE: '已执行，验证不完整',
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
  COMMAND_RESULT_UNCONFIRMED: '正式回执已返回，结果待确认。请重新读取结果，不要重复提交。',
  REVISION_CONFLICT: '作品记录在提交前发生变化，请重新读取并检查本次操作。',
  PROJECT_BUSY: '项目正在处理后台任务，请稍后再试。',
  CONTEXT_HARD_BUDGET_EXCEEDED: '本章必需资料超出预算，请调整当前任务范围后重新准备；不能省略硬性约束。',
  CONTEXT_INCOMPLETE: '本章必需资料不完整，暂时不能继续生产。',
  AUDIT_INCOMPLETE: '正文审计未完成，不能作为无变更结果提交。',
  AUDIT_STALE: '正文已变化，需重新审计最终版本。',
  CANON_CONFLICT: '正文与正式事实存在冲突，请核对依据并通过正式变更流程处理。',
  B08_RESPONSIBILITY_CHANGE_REQUEST_REQUIRED: '这项责任来自正式来源，不能直接撤销或降级；请通过变更请求提交调整。',
  B08_RESPONSIBILITY_CHANGE_REQUEST_CREATED: '已提交责任变更请求，当前责任保持不变；请在正式变更流程中确认。',
  GATE_POLICY_UPGRADE_REQUIRED: '当前仍使用旧开篇一致性规则。请先采用默认规则集，再重新编译长篇规划。',
  GATE_POLICY_RULE_INVALID: '规则格式不完整或无法确定冲突范围与时间，请修正规则后重试。',
  GATE_POLICY_CANON_CONFLICT: '新规则会与已有正式事实产生冲突，未保存。请先通过正式变更流程处理这些事实。',
  GATE_POLICY_CANON_SCAN_INCOMPLETE: '现有事实数量超出本次检查范围，规则未保存；请检查相关正式事实后重试。',
  GATE_POLICY_RULE_NOT_FOUND: '没有找到要停用的谓词规则，请刷新当前规则后重试。',
  MODEL_PRICE_REQUIRED: '已设置作品预算，但当前模型没有价格。请先填写该模型价格，未发送请求。',
  BUDGET_EXCEEDED: '本次模型调用会超出剩余预算，未发送请求。',
  FUTURE_RESERVATION_VIOLATION: '本章提前使用了后续保留的剧情内容，需要调整当前计划或正文。',
  CONTINUITY_BRIDGE_REQUIRED: '当前剧情线缺少衔接说明，暂时不能继续提交。',
  EVIDENCE_QUOTE_NOT_UNIQUE: '没有在该责任的来源正文中找到唯一引文，请补充一段更长、更有辨识度的原文。',
  UNKNOWN_REMOTE_OUTCOME: '远程请求的结果尚未确认，请先核对现有记录，不要重复发起请求。',
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
  P01_PROVIDER_EMPTY_BODY: '模型服务返回了空内容，请重试或检查服务配置。',
  P01_PROVIDER_MALFORMED_ENVELOPE: '模型服务返回格式异常，请检查服务兼容性。',
  P01_PROVIDER_CONTENT_MISSING: '模型服务没有返回可用内容，请重试。',
  P01_PROVIDER_EMPTY_CONTENT: '模型服务返回内容为空，请重试。',
  P01_MALFORMED_RESPONSE: '模型服务连接成功，但响应内容不是预期的 {"ok":true} JSON。',
  P01_MODEL_JSON_INVALID: '模型服务返回的内容不是有效的结构化结果，请重试。',
  P01_MODEL_PAYLOAD_INVALID: '模型服务返回的内容无法识别，请重试。',
  B11_STORY_DIRECTION_OUTPUT_INVALID: '模型返回的故事方向格式不完整，请重试。',
  B11_STORY_DIRECTION_COUNT_INVALID: '模型没有返回完整的三个故事方向，请重试。',
  B11_STORY_DIRECTION_ITEM_INVALID: '模型返回的故事方向内容不完整，请重试。',
  B11_STORY_DIRECTION_NOT_SUBSTANTIALLY_DIFFERENT: '模型返回的故事方向区分度不足，请重试。',
  B11_STORY_DIRECTION_FIELD_MISSING: '模型返回的故事方向缺少必要内容，请重试。',
  B11_STORY_DIRECTION_OUTPUT_MISSING: '模型没有返回故事方向，请重试。',
  B08_PROVIDER_NOT_CONFIGURED: '尚未完成模型服务配置，请先保存服务地址和访问密钥。',
}

const contextHardSourceLabels: Record<string, string> = {
  CANON: '设定与事实',
  CONTINUITY: '剧情线与责任',
  CONTRACT: '创作目标与约束',
  EVIDENCE: '依据材料',
  KNOWLEDGE: '人物认知',
  RESERVATION: '后续保留内容',
  TASK: '当前任务状态',
  UNKNOWN: '必需资料',
}

export interface ContextHardSourceSize {
  kind: string
  sourceRef: string
  selectionKind?: string | null
  selectionRef?: string | null
  fieldPath?: string | null
  additionalSourceRefs?: string[]
  additionalFieldPaths?: string[]
  estimatedTokens: number
  serializedBytes: number
  adjustable: boolean
}

export interface ContextHardBudgetFailure {
  usedTokens: number
  limitTokens: number
  serializedBytes: number
  limitBytes: number
  scope: string
  budgetPolicyVersion?: string
  contractClassificationVersion?: string
  sourceSizes: ContextHardSourceSize[]
}

export function parseContextHardBudgetFailure(value: unknown): ContextHardBudgetFailure | undefined {
  const raw = textOf(value).trim()
  const details = raw.match(/^CONTEXT_HARD_BUDGET_EXCEEDED\|details=(.+)$/)
  if (details) {
    try {
      const parsed = JSON.parse(details[1]) as Partial<ContextHardBudgetFailure>
      if (typeof parsed.usedTokens === 'number'
        && typeof parsed.limitTokens === 'number'
        && typeof parsed.serializedBytes === 'number'
        && typeof parsed.limitBytes === 'number'
        && typeof parsed.scope === 'string'
        && Array.isArray(parsed.sourceSizes)) {
        return {
          usedTokens: parsed.usedTokens,
          limitTokens: parsed.limitTokens,
          serializedBytes: parsed.serializedBytes,
          limitBytes: parsed.limitBytes,
          scope: parsed.scope,
          budgetPolicyVersion: parsed.budgetPolicyVersion,
          contractClassificationVersion: parsed.contractClassificationVersion,
          sourceSizes: parsed.sourceSizes.filter(source =>
            source && typeof source.kind === 'string'
            && typeof source.sourceRef === 'string'
            && typeof source.estimatedTokens === 'number'
            && typeof source.serializedBytes === 'number'
            && typeof source.adjustable === 'boolean',
          ).map(source => ({
            ...source,
            additionalSourceRefs: Array.isArray(source.additionalSourceRefs)
              ? source.additionalSourceRefs.filter(ref => typeof ref === 'string')
              : [],
            additionalFieldPaths: Array.isArray(source.additionalFieldPaths)
              ? source.additionalFieldPaths.filter(path => typeof path === 'string')
              : [],
          })),
        }
      }
    } catch {
      return undefined
    }
  }

  const legacy = raw.match(/CONTEXT_HARD_BUDGET_EXCEEDED\|usedTokens=(\d+)\|limitTokens=(\d+)\|serializedBytes=(\d+)\|limitBytes=(\d+)\|scope=([A-Z]+:[^"')\s}]+)/)
  if (!legacy) return undefined
  return {
    usedTokens: Number(legacy[1]),
    limitTokens: Number(legacy[2]),
    serializedBytes: Number(legacy[3]),
    limitBytes: Number(legacy[4]),
    scope: legacy[5],
    sourceSizes: [],
  }
}

function contextBlockerMessage(raw: string): string | undefined {
  const budget = parseContextHardBudgetFailure(raw)
  if (budget) {
    const scope = budget.scope.replace(/^[A-Z]+:/, '')
    return '当前范围 ' + scope + ' 的必需资料估算为 '
      + budget.usedTokens + ' / ' + budget.limitTokens + ' token，序列化数据为 '
      + budget.serializedBytes + ' / ' + budget.limitBytes
      + ' 字节，超过硬性预算；请在本章 Plan 中缩小任务、责任或实体范围后手动重新编译。系统不会自动删减或降级硬性来源。'
  }

  const incomplete = raw.match(/CONTEXT_HARD_INCOMPLETE\|sources=([A-Z_,]+)\|scope=([A-Z]+:[^"')\s}]+)/)
  if (incomplete) {
    const sources = incomplete[1]
      .split(',')
      .map(source => contextHardSourceLabels[source] ?? '必需资料')
      .join('、')
    const scope = incomplete[2].replace(/^[A-Z]+:/, '')
    return `必需资料未完整读取（${sources}，范围 ${scope}），不能继续；请缩小当前范围或补齐资料后重新准备。`
  }

  const unavailable = raw.match(/CONTEXT_SOURCE_UNAVAILABLE\|source=([A-Z_]+)\|scope=([A-Z]+:[^"')\s}]+)/)
  if (unavailable) {
    const source = contextHardSourceLabels[unavailable[1]] ?? '必需资料'
    const scope = unavailable[2].replace(/^[A-Z]+:/, '')
    return `暂时无法读取${source}（范围 ${scope}），请恢复该资料后重新准备；不能忽略后继续。`
  }
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

export function repairExecutionLabel(value: unknown): string {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return '结果待确认'
  const receipt = value as Record<string, unknown>
  const status = textOf(receipt.status).trim()
  if (status === 'REPAIR_VERIFIED') return '已执行并验证'
  if (status === 'REPAIR_VERIFICATION_INCOMPLETE') return '已执行，验证不完整'
  if (status === 'REPAIR_VERIFICATION_FAILED') {
    const postconditions = Array.isArray(receipt.postconditions) ? receipt.postconditions : []
    const ownerFailed = postconditions.some(item =>
      item && typeof item === 'object' && (item as Record<string, unknown>).status === 'OWNER_EXECUTION_FAILED',
    )
    return ownerFailed ? '执行失败' : '已执行，核验未通过'
  }
  return statusLabel(status, '结果待确认')
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
  const contextMessage = contextBlockerMessage(raw)
  if (contextMessage) return contextMessage
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

export function productTimeLabel(value: unknown): string {
  const raw = textOf(value).trim()
  if (!raw) return '暂无记录'
  const date = new Date(raw)
  if (Number.isNaN(date.getTime()) || date.getTime() <= 86_400_000) return '尚未打开'
  const now = new Date()
  const sameDay = date.getFullYear() === now.getFullYear() && date.getMonth() === now.getMonth() && date.getDate() === now.getDate()
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1)
  const isYesterday = date.getFullYear() === yesterday.getFullYear() && date.getMonth() === yesterday.getMonth() && date.getDate() === yesterday.getDate()
  const time = new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false }).format(date)
  if (sameDay) return `今天 ${time}`
  if (isYesterday) return `昨天 ${time}`
  return `${date.getFullYear()}年${date.getMonth() + 1}月${date.getDate()}日 ${time}`
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
  if (record.actionStatus === 'CHANGE_REQUEST_SUBMITTED') {
    return {
      status: 'success',
      title: '已提交变更请求',
      detail: productErrorMessage(record.message, '责任变更请求已提交，当前责任保持不变。'),
    }
  }
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
