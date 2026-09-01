import creativeSkillsManifest from '../../../creative-skills/v1.2.0/registry.json'

export const CREATIVE_SKILLS_VERSION = '1.2.0'
export const CREATIVE_SKILLS_ARCHIVE_SHA256 = 'a90ab6e12d82d57b32f06d5a3780d0153d6f372e1efd47df1d70ef1aebbdef8f'
export const CREATIVE_SKILL_MAX_DEFAULT = 4
export const CREATIVE_SKILL_MAX_WITH_BUDGET = 8

export const SKILL_OUTPUT_KINDS = [
  'CreativeProposal',
  'Candidate',
  'CandidatePatch',
  'QualityFinding',
  'RepairSuggestion',
  'EscalationAdvice',
] as const

export type SkillOutputKind = typeof SKILL_OUTPUT_KINDS[number]
export type SkillOutputStatus = 'OK' | 'NEEDS_INPUT' | 'NOT_APPLICABLE' | 'ADVISORY_ESCALATION'

export const RUNTIME_CHECK_KINDS = [
  'CANON_CONFLICT',
  'KNOWLEDGE_BOUNDARY',
  'THREAD_STATE',
  'TASK_REALITY',
  'RESERVATION',
  'INTEGRITY',
  'PLAN_STALE',
  'EVIDENCE_GAP',
] as const

export type RuntimeCheckKind = typeof RUNTIME_CHECK_KINDS[number]

export const FORBIDDEN_SKILL_CAPABILITIES = [
  'DB_QUERY',
  'SQL',
  'REPOSITORY_READ',
  'AUTHORITY_WRITE',
  'COMMIT',
  'TASK_REALITY_WRITE',
  'THREAD_WRITE',
  'RESERVATION_ENFORCEMENT',
  'ROUTE_REPLAN',
  'CHANGE_EXECUTION',
  'H03_P3',
  'HARD_GATE',
] as const

export type ForbiddenSkillCapability = typeof FORBIDDEN_SKILL_CAPABILITIES[number]

export interface SkillMetadata {
  id: string
  slug: string
  nameZh: string
  category: string
  priority: number
  role: string
  runtimeStage: string
  contextPolicy: string
  mutationPolicy: string
  coverage: string[]
  bundleDigest: string
  authorityAccess: string
  hardGateAuthority: boolean
  controlDecisionAuthority: boolean
}

type RegistryShape = {
  package: string
  version: string
  skillCount: number
  selectionPolicy: string
  skills: Array<Record<string, unknown>>
}

const registryShape = creativeSkillsManifest as RegistryShape

const stringList = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []

const toSkillMetadata = (value: Record<string, unknown>): SkillMetadata => ({
  id: String(value.id ?? ''),
  slug: String(value.slug ?? ''),
  nameZh: String(value.name_zh ?? value.nameZh ?? ''),
  category: String(value.category ?? ''),
  priority: Number(value.priority ?? 0),
  role: String(value.role ?? ''),
  runtimeStage: String(value.runtimeStage ?? ''),
  contextPolicy: String(value.contextPolicy ?? ''),
  mutationPolicy: String(value.mutationPolicy ?? ''),
  coverage: stringList(value.coverage),
  bundleDigest: String(value.bundleDigest ?? ''),
  authorityAccess: String(value.authorityAccess ?? 'NONE_DIRECT'),
  hardGateAuthority: value.hardGateAuthority === true,
  controlDecisionAuthority: value.controlDecisionAuthority === true,
})

export const CREATIVE_SKILL_REGISTRY = Object.freeze({
  package: registryShape.package,
  version: registryShape.version,
  skillCount: registryShape.skillCount,
  selectionPolicy: registryShape.selectionPolicy,
  skills: Object.freeze(registryShape.skills.map(toSkillMetadata)),
})

export function getCreativeSkillRegistry() {
  return CREATIVE_SKILL_REGISTRY
}

export interface SkillSelectionRequest {
  skills: readonly SkillMetadata[]
  primary: string
  support?: readonly string[]
  budgetReason?: string
  budget?: { contextTokens: number; modelCalls: number }
}

export interface SkillSelection {
  primary: SkillMetadata
  support: SkillMetadata[]
  selected: SkillMetadata[]
  budgetReason?: string
  budget?: { contextTokens: number; modelCalls: number }
}

export function selectCreativeSkills(request: SkillSelectionRequest): SkillSelection {
  const byId = new Map(request.skills.map(skill => [skill.id, skill]))
  const supportIds = [...(request.support ?? [])]
  const selectedIds = [request.primary, ...supportIds]
  if (!byId.has(request.primary)) throw new Error('SKILL_PRIMARY_NOT_FOUND')
  if (new Set(selectedIds).size !== selectedIds.length) throw new Error('SKILL_SELECTION_DUPLICATE')
  const selected = selectedIds.map(id => {
    const skill = byId.get(id)
    if (!skill) throw new Error(`SKILL_NOT_FOUND:${id}`)
    return skill
  })
  if (selected.length > CREATIVE_SKILL_MAX_WITH_BUDGET) throw new Error('SKILL_SELECTION_UNBOUNDED')
  if (selected.length > CREATIVE_SKILL_MAX_DEFAULT) {
    const hasReason = typeof request.budgetReason === 'string' && request.budgetReason.trim().length > 0
    const hasBudget = Number.isInteger(request.budget?.contextTokens) && (request.budget?.contextTokens ?? 0) > 0
      && Number.isInteger(request.budget?.modelCalls) && (request.budget?.modelCalls ?? 0) > 0
    if (!hasReason || !hasBudget) throw new Error('SKILL_SELECTION_BUDGET_REQUIRED')
  }
  return {
    primary: selected[0],
    support: selected.slice(1),
    selected,
    budgetReason: request.budgetReason,
    budget: request.budget,
  }
}

export interface LoadedSkillResource {
  text: string
  digest: string
}

export interface SkillBundleSource {
  loadSkillText(skill: SkillMetadata): Promise<LoadedSkillResource>
  loadResource(skill: SkillMetadata, resourcePath: string): Promise<LoadedSkillResource>
}

export interface SkillLoadingEvidence {
  metadataLoads: number
  selectedFullTextLoads: number
  unselectedFullTextLoads: number
  selectedResourceLoads: number
  unselectedResourceLoads: number
  selectedSkillIds: string[]
  loadedSkillIds: string[]
  loadedResources: string[]
}

export class CreativeSkillLoader {
  private metadataLoads = 0
  private readonly selectedById: Map<string, SkillMetadata>
  private readonly loadedSkillIds = new Set<string>()
  private readonly loadedResourceKeys = new Set<string>()
  private readonly unselectedSkillAttempts = new Set<string>()
  private readonly unselectedResourceAttempts = new Set<string>()

  constructor(private readonly source: SkillBundleSource, selectedSkills: readonly SkillMetadata[]) {
    this.selectedById = new Map(selectedSkills.map(skill => [skill.id, skill]))
  }

  async loadMetadata(): Promise<SkillMetadata[]> {
    this.metadataLoads += 1
    return [...this.selectedById.values()]
  }

  async loadSkillText(skillId: string): Promise<LoadedSkillResource> {
    const skill = this.selectedById.get(skillId)
    if (!skill) {
      this.unselectedSkillAttempts.add(skillId)
      throw new Error('SKILL_NOT_SELECTED')
    }
    const loaded = await this.source.loadSkillText(skill)
    this.loadedSkillIds.add(skillId)
    return loaded
  }

  async loadResource(skillId: string, resourcePath: string): Promise<LoadedSkillResource> {
    const skill = this.selectedById.get(skillId)
    if (!skill) {
      this.unselectedResourceAttempts.add(`${skillId}/${resourcePath}`)
      throw new Error('SKILL_NOT_SELECTED')
    }
    if (!this.loadedSkillIds.has(skillId)) throw new Error('SKILL_TEXT_REQUIRED_BEFORE_RESOURCE')
    const loaded = await this.source.loadResource(skill, resourcePath)
    this.loadedResourceKeys.add(`${skillId}/${resourcePath}`)
    return loaded
  }

  evidence(): SkillLoadingEvidence {
    return {
      metadataLoads: this.metadataLoads,
      selectedFullTextLoads: this.loadedSkillIds.size,
      unselectedFullTextLoads: this.unselectedSkillAttempts.size,
      selectedResourceLoads: this.loadedResourceKeys.size,
      unselectedResourceLoads: this.unselectedResourceAttempts.size,
      selectedSkillIds: [...this.selectedById.keys()],
      loadedSkillIds: [...this.loadedSkillIds],
      loadedResources: [...this.loadedResourceKeys],
    }
  }
}

type GlobLoader = () => Promise<unknown>

const browserSkillTextLoaders = import.meta.glob('../../../creative-skills/v1.2.0/skills/*/SKILL.md', {
  query: '?raw',
  import: 'default',
}) as Record<string, GlobLoader>

const browserResourceLoaders = import.meta.glob('../../../creative-skills/v1.2.0/skills/*/resources/*', {
  query: '?raw',
  import: 'default',
}) as Record<string, GlobLoader>

const unwrapGlobValue = (value: unknown): string => {
  if (typeof value === 'string') return value
  if (value && typeof value === 'object' && 'default' in value && typeof value.default === 'string') return value.default
  throw new Error('SKILL_RESOURCE_INVALID')
}

const findGlobLoader = (loaders: Record<string, GlobLoader>, suffix: string): GlobLoader => {
  const key = Object.keys(loaders).find(candidate => candidate.endsWith(suffix))
  if (!key) throw new Error(`SKILL_RESOURCE_NOT_FOUND:${suffix}`)
  return loaders[key]
}

export function createBrowserSkillBundleSource(): SkillBundleSource {
  return {
    async loadSkillText(skill) {
      const text = unwrapGlobValue(await findGlobLoader(browserSkillTextLoaders, `/skills/${skill.slug}/SKILL.md`)())
      return { text, digest: skill.bundleDigest.replace(/^sha256:/, '') }
    },
    async loadResource(skill, resourcePath) {
      const text = unwrapGlobValue(await findGlobLoader(browserResourceLoaders, `/skills/${skill.slug}/resources/${resourcePath}`)())
      return { text, digest: skill.bundleDigest.replace(/^sha256:/, '') }
    },
  }
}

export interface BoundedRef {
  kind: string
  ref: string
}

export interface ContextCapsule {
  kind: string
  ref: string
  summary: string
  digest: string
}

export interface SkillTaskContext {
  taskId: string
  projectId: string
  projectRevision: number
  runtimeStage: string
  contextManifestRef: string
  boundedRefs: BoundedRef[]
  capsules: ContextCapsule[]
  qualityInputs: Record<string, unknown>
  methodProfile: Record<string, unknown>
  budget: { contextTokens: number; modelCalls: number; repairRuns: number }
  allowedOutputs: SkillOutputKind[]
  forbiddenCapabilities: ForbiddenSkillCapability[]
  contextDigest: string
  assertFresh(currentRevision: number): { status: 'FRESH' }
}

export type SkillTaskContextInput = Omit<SkillTaskContext, 'contextDigest' | 'allowedOutputs' | 'forbiddenCapabilities' | 'assertFresh'> & {
  allowedOutputs?: readonly SkillOutputKind[]
  forbiddenCapabilities?: readonly ForbiddenSkillCapability[]
}

const MAX_CONTEXT_BYTES = 64_000
const MAX_REFS = 64
const MAX_CAPSULES = 16
const MAX_CAPSULE_SUMMARY = 2_000
const DISALLOWED_CONTEXT_KEYS = new Set([
  'rawProjectText',
  'fullProjectText',
  'projectDump',
  'database',
  'sql',
  'repo',
  'repository',
])

function stableStringify(value: unknown): string {
  if (value === null || typeof value !== 'object') return JSON.stringify(value)
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(',')}]`
  const entries = Object.entries(value as Record<string, unknown>).sort(([left], [right]) => left.localeCompare(right))
  return `{${entries.map(([key, child]) => `${JSON.stringify(key)}:${stableStringify(child)}`).join(',')}}`
}

// UI-side fingerprints keep the context contract deterministic without pulling
// a Node crypto dependency into the browser bundle. Authoritative persisted
// artifact hashes continue to be calculated by the Rust CommitService.
function stableFingerprint(value: unknown): string {
  const text = stableStringify(value)
  const words = [0x811c9dc5, 0x9e3779b1, 0x85ebca6b, 0xc2b2ae35, 0x27d4eb2f, 0x165667b1, 0xd3a2646c, 0xfd7046c5]
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index)
    for (let word = 0; word < words.length; word += 1) {
      words[word] ^= (code + word * 17) & 0xff
      words[word] = Math.imul(words[word], 0x01000193) >>> 0
    }
  }
  return words.map(word => word.toString(16).padStart(8, '0')).join('')
}

function validateNonEmpty(value: unknown, code: string): string {
  if (typeof value !== 'string' || value.trim().length === 0) throw new Error(code)
  return value
}

export function createSkillTaskContext(input: SkillTaskContextInput): SkillTaskContext {
  const inputRecord = input as unknown as Record<string, unknown>
  if (Object.keys(inputRecord).some(key => DISALLOWED_CONTEXT_KEYS.has(key))) throw new Error('SKILL_CONTEXT_UNBOUNDED_INPUT')
  const projectRevision = input.projectRevision
  if (!Number.isInteger(projectRevision) || projectRevision < 0) throw new Error('SKILL_CONTEXT_REVISION_INVALID')
  const boundedRefs = input.boundedRefs.map(ref => ({
    kind: validateNonEmpty(ref.kind, 'SKILL_CONTEXT_REF_KIND_INVALID'),
    ref: validateNonEmpty(ref.ref, 'SKILL_CONTEXT_REF_INVALID'),
  }))
  if (boundedRefs.length > MAX_REFS) throw new Error('SKILL_CONTEXT_REF_LIMIT')
  const capsules = input.capsules.map(capsule => ({
    kind: validateNonEmpty(capsule.kind, 'SKILL_CONTEXT_CAPSULE_KIND_INVALID'),
    ref: validateNonEmpty(capsule.ref, 'SKILL_CONTEXT_CAPSULE_REF_INVALID'),
    summary: validateNonEmpty(capsule.summary, 'SKILL_CONTEXT_CAPSULE_SUMMARY_INVALID'),
    digest: validateNonEmpty(capsule.digest, 'SKILL_CONTEXT_CAPSULE_DIGEST_INVALID'),
  }))
  if (capsules.length > MAX_CAPSULES || capsules.some(capsule => capsule.summary.length > MAX_CAPSULE_SUMMARY)) throw new Error('SKILL_CONTEXT_CAPSULE_LIMIT')
  const budget = {
    contextTokens: input.budget.contextTokens,
    modelCalls: input.budget.modelCalls,
    repairRuns: input.budget.repairRuns,
  }
  if (Object.values(budget).some(value => !Number.isInteger(value) || value < 0)) throw new Error('SKILL_CONTEXT_BUDGET_INVALID')
  const allowedOutputs = [...(input.allowedOutputs ?? SKILL_OUTPUT_KINDS)]
  if (allowedOutputs.some(output => !SKILL_OUTPUT_KINDS.includes(output))) throw new Error('SKILL_OUTPUT_KIND_INVALID')
  const forbiddenCapabilities = [...new Set([
    ...FORBIDDEN_SKILL_CAPABILITIES,
    ...(input.forbiddenCapabilities ?? []),
  ])] as ForbiddenSkillCapability[]
  const base = {
    taskId: validateNonEmpty(input.taskId, 'SKILL_CONTEXT_TASK_INVALID'),
    projectId: validateNonEmpty(input.projectId, 'SKILL_CONTEXT_PROJECT_INVALID'),
    projectRevision,
    runtimeStage: validateNonEmpty(input.runtimeStage, 'SKILL_CONTEXT_STAGE_INVALID'),
    contextManifestRef: validateNonEmpty(input.contextManifestRef, 'SKILL_CONTEXT_MANIFEST_INVALID'),
    boundedRefs,
    capsules,
    qualityInputs: input.qualityInputs,
    methodProfile: input.methodProfile,
    budget,
    allowedOutputs,
    forbiddenCapabilities,
  }
  if (new TextEncoder().encode(stableStringify(base)).length > MAX_CONTEXT_BYTES) throw new Error('SKILL_CONTEXT_BYTES_LIMIT')
  return {
    ...base,
    contextDigest: stableFingerprint(base),
    assertFresh(currentRevision) {
      if (currentRevision !== projectRevision) throw new Error('STALE/RECOMPILE')
      return { status: 'FRESH' }
    },
  }
}

export interface RuntimeCheckRequest {
  kind: RuntimeCheckKind
  detail?: string
  refs?: string[]
}

export interface NormalizedSkillOutput {
  kind: SkillOutputKind
  status: SkillOutputStatus
  summary: string
  suggestions: string[]
  runtimeCheckRequests: RuntimeCheckRequest[]
  safety: { formalBlock: false; authorityWrite: false; controlDecision: false }
}

export function normalizeSkillOutput(raw: unknown): NormalizedSkillOutput {
  if (!raw || typeof raw !== 'object') throw new Error('SKILL_OUTPUT_INVALID')
  const value = raw as Record<string, unknown>
  const kind = value.kind
  if (typeof kind !== 'string' || !SKILL_OUTPUT_KINDS.includes(kind as SkillOutputKind)) throw new Error('SKILL_OUTPUT_KIND_INVALID')
  const rawStatus = value.status
  const status: SkillOutputStatus = rawStatus === 'BLOCK'
    ? 'ADVISORY_ESCALATION'
    : rawStatus === undefined
      ? 'OK'
      : rawStatus as SkillOutputStatus
  if (!['OK', 'NEEDS_INPUT', 'NOT_APPLICABLE', 'ADVISORY_ESCALATION'].includes(status)) throw new Error('SKILL_OUTPUT_STATUS_INVALID')
  const rawRequests = value.runtimeCheckRequests
  if (rawRequests !== undefined && !Array.isArray(rawRequests)) throw new Error('SKILL_RUNTIME_CHECKS_INVALID')
  const runtimeCheckRequests = (rawRequests ?? []).map(item => {
    if (!item || typeof item !== 'object') throw new Error('SKILL_RUNTIME_CHECK_INVALID')
    const request = item as Record<string, unknown>
    if (typeof request.kind !== 'string' || !RUNTIME_CHECK_KINDS.includes(request.kind as RuntimeCheckKind)) throw new Error('SKILL_RUNTIME_CHECK_KIND_INVALID')
    const normalized: RuntimeCheckRequest = { kind: request.kind as RuntimeCheckKind }
    if (typeof request.detail === 'string') normalized.detail = request.detail
    if (Array.isArray(request.refs)) normalized.refs = request.refs.filter((ref): ref is string => typeof ref === 'string').slice(0, MAX_REFS)
    return normalized
  })
  if (runtimeCheckRequests.length > 8) throw new Error('SKILL_RUNTIME_CHECK_LIMIT')
  return {
    kind: kind as SkillOutputKind,
    status,
    summary: typeof value.summary === 'string' ? value.summary.slice(0, 4_000) : '',
    suggestions: Array.isArray(value.suggestions)
      ? value.suggestions.filter((suggestion): suggestion is string => typeof suggestion === 'string').slice(0, 16)
      : [],
    runtimeCheckRequests,
    safety: { formalBlock: false, authorityWrite: false, controlDecision: false },
  }
}

export interface InvocationProvenance {
  invocationId: string
  skillId: string
  skillVersion: string
  bundleDigest: string
  role: string
  resourceRefs: string[]
  resourceDigests: Array<{ ref: string; digest: string }>
  contextRef: string
  contextDigest: string
  contextManifestRef: string
  methodRefs: string[]
  projectContextRefs: string[]
  sourceRefs: string[]
  runtimeCheckRequests: RuntimeCheckRequest[]
  projectId: string
  projectRevision: number
  model: string
  provider: string
  modelRunRefs: string[]
  tokens: { input: number; output: number }
  costCents: number
  outputDigest: string
  createdAt: string
}

export function createInvocationProvenance(input: {
  invocationId: string
  skill: SkillMetadata
  context: SkillTaskContext
  methodRefs: readonly string[]
  projectContextRefs: readonly string[]
  resourceRefs?: readonly (string | { ref: string; digest: string })[]
  runtimeCheckRequests?: readonly RuntimeCheckRequest[]
  model: string
  provider: string
  modelRunRefs?: readonly string[]
  tokens: { input: number; output: number }
  costCents: number
  outputDigest: string
  createdAt?: string
}): InvocationProvenance {
  const resourceDigests = (input.resourceRefs ?? []).map(resource => typeof resource === 'string'
    ? { ref: resource, digest: '' }
    : { ref: resource.ref, digest: resource.digest })
  return {
    invocationId: validateNonEmpty(input.invocationId, 'SKILL_INVOCATION_ID_INVALID'),
    skillId: input.skill.id,
    skillVersion: CREATIVE_SKILLS_VERSION,
    bundleDigest: input.skill.bundleDigest,
    role: input.skill.role,
    resourceRefs: resourceDigests.map(resource => resource.ref),
    resourceDigests,
    contextRef: input.context.contextManifestRef,
    contextDigest: input.context.contextDigest,
    contextManifestRef: input.context.contextManifestRef,
    methodRefs: [...input.methodRefs],
    projectContextRefs: [...input.projectContextRefs],
    sourceRefs: [...input.methodRefs],
    runtimeCheckRequests: [...(input.runtimeCheckRequests ?? [])],
    projectId: input.context.projectId,
    projectRevision: input.context.projectRevision,
    model: input.model,
    provider: input.provider,
    modelRunRefs: [...(input.modelRunRefs ?? [])],
    tokens: input.tokens,
    costCents: input.costCents,
    outputDigest: input.outputDigest,
    createdAt: input.createdAt ?? new Date().toISOString(),
  }
}

export interface TasteProfile {
  version: string
  digest: string
  profile: Record<string, unknown>
  provenance: Record<string, unknown>
  softOnly: true
}

export function createTasteProfile(input: { version: string; profile: Record<string, unknown>; provenance: Record<string, unknown> }): TasteProfile {
  const base = { version: validateNonEmpty(input.version, 'TASTE_PROFILE_VERSION_INVALID'), profile: input.profile, provenance: input.provenance }
  return { ...base, digest: stableFingerprint(base), softOnly: true }
}

export const QUALITY_DIMENSIONS = [
  'agency',
  'desire',
  'hook',
  'pacing',
  'voice',
  'dialogue',
  'subtext',
  'repetition',
  'exposition',
  'emotionalPayoff',
] as const

export interface QualityReport {
  candidateRef: { artifactId: string; version: number; payloadHash: string }
  projectId: string
  revision: number
  tasteProfileRef: string
  tasteProfileVersion: string
  dimensions: Record<typeof QUALITY_DIMENSIONS[number], { score: number; severity: 'INFO' | 'WARN'; evidence: string[]; signal: string; evidenceRanges: Array<{ start: number; end: number; quote: string }> }>
  findings: Array<{ findingId: string; dimension: string; severity: 'INFO' | 'WARN'; evidence: string[]; repairable: boolean; reason: string; evidenceRanges: Array<{ start: number; end: number; quote: string }> }>
  repairableDimensions: string[]
  runtimeChecks: RuntimeCheckRequest[]
  softAssessment: { readerDesire: 'SOFT'; qualityScore: number }
  modelProvenance: { evaluator: string; evaluatorCalls: number; provider: string; model: string }
  invocationRefs: string[]
  recheckedDimensions: string[]
  digest: string
  softOnly: true
}

export function evaluateNarrativeQuality(input: {
  candidateRef: { artifactId: string; version: number; payloadHash: string }
  projectId: string
  revision: number
  body: string
  tasteProfile: TasteProfile
  invocationRefs: readonly string[]
}): QualityReport {
  const countAny = (needles: string[]) => needles.reduce((count, needle) => count + [...input.body.matchAll(new RegExp(needle.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gu'))].length, 0)
  const firstRange = (needles: string[]) => {
    for (const needle of needles) {
      const start = input.body.indexOf(needle)
      if (start >= 0) return [{ start, end: start + [...needle].length, quote: needle }]
    }
    return []
  }
  const dimension = (score: number, signal: string, needles: string[]) => {
    const bounded = Math.max(0, Math.min(100, Math.round(score)))
    const evidenceRanges = firstRange(needles)
    return {
      score: bounded,
      severity: (bounded < 45 ? 'WARN' : 'INFO') as 'INFO' | 'WARN',
      evidence: evidenceRanges.length > 0 ? evidenceRanges.map(range => `${range.quote}:${range.start}-${range.end}`) : [`signal:${signal}`],
      signal,
      evidenceRanges,
    }
  }
  const sentences = input.body.split(/[。！？!?]/u).map(sentence => sentence.trim()).filter(Boolean)
  const sentenceCounts = new Map<string, number>()
  for (const sentence of sentences) sentenceCounts.set(sentence, (sentenceCounts.get(sentence) ?? 0) + 1)
  const duplicateSentences = [...sentenceCounts.values()].reduce((count, value) => count + Math.max(0, value - 1), 0)
  const initiative = countAny(['决定', '选择', '推开', '拔刀', '冲向', '走向', '拒绝', '发誓', '主动', 'decide', 'choose'])
  const passive = countAny(['被', '只能', '等待', '任由', '陷入', '被迫', 'passively'])
  const desire = countAny(['想', '要', '必须', '希望', '渴望', '目标', '为了', 'want', 'must'])
  const unresolved = countAny(['？', '?', '却', '突然', '门后', '谁', '为何', '秘密', '传来', '然而', '但'])
  const closure = countAny(['结束', '完成', '关闭', '安静地结束', '终于平静'])
  const actions = countAny(['推', '走', '跑', '拔', '抓', '打', '看', '打开', '冲', '决定'])
  const dialoguePairs = (input.body.match(/“/gu)?.length ?? 0) + Math.floor((input.body.match(/"/gu)?.length ?? 0) / 2)
  const subtext = countAny(['沉默', '没有说', '却笑', '眼神', '停顿', '其实', '不敢'])
  const exposition = countAny(['所谓', '也就是说', '历史', '制度', '地理', '定义', '资料', '因为', '据说'])
  const voice = typeof input.tasteProfile.profile.voice === 'string' ? input.tasteProfile.profile.voice : '默认'
  const pacing = typeof input.tasteProfile.profile.pacing === 'string' ? input.tasteProfile.profile.pacing : '默认'
  const sentenceVariation = sentences.slice(1).filter((sentence, index) => [...sentence].length !== [...sentences[index]].length).length
  const emotion = countAny(['害怕', '愤怒', '痛', '笑', '哭', '失去', '终于', '代价', '心'])
  const dimensions = {
    agency: dimension(45 + Math.min(36, initiative * 12) - Math.min(45, passive * 15), `initiative=${initiative};passive=${passive}`, ['决定', '选择', '推开', '被', '只能']),
    desire: dimension(35 + Math.min(60, desire * 15), `desireMarkers=${desire}`, ['想', '要', '必须', '希望', '目标', '为了']),
    hook: dimension(42 + Math.min(48, unresolved * 12) - Math.min(36, closure * 18), `unresolved=${unresolved};closure=${closure}`, ['？', '?', '却', '突然', '门后', '传来', '结束']),
    pacing: dimension(35 + Math.min(35, actions * 8) + Math.min(20, sentenceVariation * 5) + (pacing.includes('紧') ? 5 : 0), `sentences=${sentences.length};actions=${actions};variation=${sentenceVariation};profile=${pacing}`, ['推', '走', '跑', '突然']),
    voice: dimension(48 + Math.min(12, (input.body.match(/，/gu)?.length ?? 0)) + (voice.includes('克制') ? 12 : voice.includes('华丽') ? -8 : 0), `tasteVoice=${voice}`, ['，', '。', '！', '？']),
    dialogue: dimension(20 + Math.min(70, dialoguePairs * 22), `dialoguePairs=${dialoguePairs}`, ['“', '"', '说', '回答']),
    subtext: dimension(35 + Math.min(60, subtext * 15), `subtextMarkers=${subtext}`, ['沉默', '没有说', '眼神', '其实', '不敢']),
    repetition: dimension(100 - Math.min(85, duplicateSentences * 25), `sentences=${sentences.length};duplicateSentences=${duplicateSentences}`, ['门后传来脚步声', '重复']),
    exposition: dimension(75 - Math.min(55, exposition * 12) - (dialoguePairs === 0 && input.body.length > 24 ? 12 : 0) + (dialoguePairs > 0 ? 8 : 0), `expositionMarkers=${exposition};dialoguePairs=${dialoguePairs}`, ['所谓', '历史', '制度', '地理', '也就是说', '资料']),
    emotionalPayoff: dimension(35 + Math.min(35, emotion * 10) + Math.min(24, initiative * 8) + (closure > 0 ? 8 : 0), `emotionMarkers=${emotion};causalActions=${initiative};closure=${closure}`, ['终于', '代价', '决定', '笑', '哭', '失去']),
  } satisfies QualityReport['dimensions']
  const findings = Object.entries(dimensions)
    .filter(([, item]) => item.score < 45)
    .map(([dimensionName, item]) => ({
      findingId: `quality:${input.candidateRef.artifactId}:${dimensionName}`,
      dimension: dimensionName,
      severity: item.severity,
      evidence: item.evidence,
      repairable: ['agency', 'hook', 'dialogue', 'pacing', 'repetition', 'exposition'].includes(dimensionName),
      reason: item.signal,
      evidenceRanges: item.evidenceRanges,
    }))
  const qualityScore = Math.round(Object.values(dimensions).reduce((sum, item) => sum + item.score, 0) / QUALITY_DIMENSIONS.length)
  const base = {
    candidateRef: input.candidateRef,
    projectId: input.projectId,
    revision: input.revision,
    tasteProfileRef: input.tasteProfile.digest,
    tasteProfileVersion: input.tasteProfile.version,
    dimensions,
    findings,
    repairableDimensions: findings.filter(finding => finding.repairable).map(finding => finding.dimension),
    runtimeChecks: [],
    softAssessment: { readerDesire: 'SOFT' as const, qualityScore },
    modelProvenance: { evaluator: 'B10::MergedNarrativeEvaluator', evaluatorCalls: 1, provider: 'deterministic', model: 'quality-rules-v1' },
    invocationRefs: [...input.invocationRefs],
    recheckedDimensions: [],
    softOnly: true as const,
  }
  return { ...base, digest: stableFingerprint({ ...base, invocationRefs: [...input.invocationRefs] }) }
}

export function recheckAffectedDimensions(
  prior: QualityReport,
  repairedBody: string,
  tasteProfile: TasteProfile,
  affectedDimensions: readonly string[],
): QualityReport {
  const affected = [...new Set(affectedDimensions)].sort()
  if (affected.some(dimension => !(dimension in prior.dimensions))) throw new Error('SKILL_AFFECTED_DIMENSION_INVALID')
  const fresh = evaluateNarrativeQuality({
    candidateRef: prior.candidateRef,
    projectId: prior.projectId,
    revision: prior.revision,
    body: repairedBody,
    tasteProfile,
    invocationRefs: prior.invocationRefs,
  })
  const dimensions = { ...prior.dimensions }
  for (const dimension of affected) dimensions[dimension as keyof typeof dimensions] = fresh.dimensions[dimension as keyof typeof dimensions]
  const findings = [
    ...prior.findings.filter(finding => !affected.includes(finding.dimension)),
    ...fresh.findings.filter(finding => affected.includes(finding.dimension)),
  ]
  const next: QualityReport = {
    ...prior,
    dimensions,
    findings,
    repairableDimensions: findings.filter(finding => finding.repairable).map(finding => finding.dimension),
    recheckedDimensions: affected,
  }
  return { ...next, digest: stableFingerprint({ ...next, digest: undefined }) }
}

export function enforceHardQualitySeparation(input: { quality: QualityReport; hardGate: { passed: boolean; reason?: string } }): {
  decision: 'PASS' | 'BLOCKED'
  hardReason?: string
  qualityAction: 'NO_REPAIR' | 'REPAIR_OR_ESCALATE'
} {
  if (!input.hardGate.passed) return { decision: 'BLOCKED', hardReason: input.hardGate.reason ?? 'HARD_GATE_FAILED', qualityAction: 'NO_REPAIR' }
  return {
    decision: 'PASS',
    qualityAction: input.quality.findings.length > 0 ? 'REPAIR_OR_ESCALATE' : 'NO_REPAIR',
  }
}

export interface RepairCandidateInput {
  candidate: { status: 'CANDIDATE' | 'COMMITTED' | 'FINAL'; body: string; candidateDigest: string; candidateRef: string }
  findingRefs: readonly string[]
  affectedDimensions: readonly string[]
  patch: { start: number; end: number; replacement: string }
  skillId: string
  skillVersion: string
  model: string
  repairRun: number
  repairRunLimit: number
  contextTokens: number
  modelCalls: number
}

export type RepairCandidateResult = {
  kind: 'CandidatePatch' | 'RepairSuggestion'
  status: 'CANDIDATE' | 'ADVISORY_ESCALATION'
  body?: string
  candidateRef: string
  candidateDigest?: string
  findingRefs: string[]
  affectedDimensions: string[]
  repairRun: number
  repairRunLimit: number
  transportCalls: number
  reason?: string
  model?: string
  skillId?: string
  skillVersion?: string
}

export function repairCandidate(input: RepairCandidateInput): RepairCandidateResult {
  if (input.candidate.status !== 'CANDIDATE') throw new Error('SKILL_CANDIDATE_ONLY')
  if (input.repairRunLimit < 1 || input.repairRun < 1) throw new Error('SKILL_REPAIR_LIMIT_INVALID')
  if (input.repairRun >= input.repairRunLimit) {
    return {
      kind: 'RepairSuggestion',
      status: 'ADVISORY_ESCALATION',
      candidateRef: input.candidate.candidateRef,
      findingRefs: [...input.findingRefs],
      affectedDimensions: [...input.affectedDimensions],
      repairRun: input.repairRun,
      repairRunLimit: input.repairRunLimit,
      transportCalls: 0,
      reason: 'REPAIR_RUN_LIMIT',
    }
  }
  const deterministic = input.model.startsWith('deterministic-')
  if (input.contextTokens <= 0 || (!deterministic && input.modelCalls <= 0)) throw new Error('SKILL_REPAIR_BUDGET_INVALID')
  const characters = [...input.candidate.body]
  if (!Number.isInteger(input.patch.start) || !Number.isInteger(input.patch.end)
    || input.patch.start < 0 || input.patch.start > input.patch.end || input.patch.end > characters.length) {
    throw new Error('SKILL_PATCH_RANGE_INVALID')
  }
  const body = characters.slice(0, input.patch.start).join('')
    + input.patch.replacement
    + characters.slice(input.patch.end).join('')
  return {
    kind: 'CandidatePatch',
    status: 'CANDIDATE',
    body,
    candidateRef: input.candidate.candidateRef,
    candidateDigest: stableFingerprint(body),
    findingRefs: [...input.findingRefs],
    affectedDimensions: [...input.affectedDimensions],
    repairRun: input.repairRun,
    repairRunLimit: input.repairRunLimit,
    transportCalls: deterministic ? 0 : 1,
    model: input.model,
    skillId: input.skillId,
    skillVersion: input.skillVersion,
  }
}
