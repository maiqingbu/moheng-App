<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import { getFrontendPort } from '../../app/providers/runtimePort'
import type { CommandStatus, CompletionStatusProjection, EndgameMappingWorkflowProjection, SoftCreativeAdviceProjection } from '../../application/contracts'
import { productErrorMessage } from '../presentation/productLanguage'
import { scopedMappingCandidates } from '../../application/endgameMapping'

const route = useRoute()
const frontendPort = getFrontendPort()
const projectId = computed(() => typeof route.params.id === 'string' ? route.params.id : '')
const status = ref<CompletionStatusProjection | null>(null)
const summary = ref('')
const endingConfirmed = ref(false)
const completionConfirmed = ref(false)
const loading = ref(false)
const submitting = ref(false)
const error = ref('')
const message = ref('')
const resultPending = ref(false)
const committedReceiptId = ref('')
const messageKind = ref<'info' | 'success' | 'warning'>('info')
const endgameAdvice = ref<SoftCreativeAdviceProjection | null>(null)
const qualityReview = ref<SoftCreativeAdviceProjection | null>(null)
const mappingWorkflow = ref<EndgameMappingWorkflowProjection | null>(null)
const mappingSelections = ref<Record<string, string>>({})
const mappingLoading = ref(false)

const statusText = computed(() => {
  switch (status.value?.status) {
    case 'READY': return '可以进入完结提交'
    case 'BLOCKED': return '还有正式阻断项'
    case 'NEEDS_DECISION': return '需要你的明确决定'
    case 'NEEDS_RECHECK': return '完结确认已过期，需要重新检查'
    case 'SOURCE_UNAVAILABLE': return '正式记录暂时不可读'
    default: return '等待正式检查'
  }
})

const statusClass = computed(() => status.value?.status === 'READY' ? 'success' : status.value?.status === 'NEEDS_DECISION' || status.value?.status === 'NEEDS_RECHECK' ? 'warning' : 'danger')
const canComplete = computed(() => status.value?.status === 'READY' && !status.value.completed && !resultPending.value && !loading.value && !error.value)

function checkName(kind: string) {
  return ({ OBLIGATION: '正式剧情责任', MYSTERY: '重要谜团', PAYOFF: '关键回报', REQUESTED_ENDING: '用户确认的结局', CHANGE_STALE: '变更与规划', EXPORT_INTEGRITY: '成品完整性' } as Record<string, string>)[kind] ?? '完结条件'
}
function checkStatus(status: string) {
  return ({ READY: '已满足', BLOCKED: '还有必须解决的问题', NEEDS_DECISION: '需要先确认', NEEDS_RECHECK: '需要重新检查', SOURCE_UNAVAILABLE: '暂时无法完成检查' } as Record<string, string>)[status] ?? '需要重新检查'
}
function checkClass(status: string) { return status === 'READY' ? 'success' : status === 'NEEDS_DECISION' || status === 'NEEDS_RECHECK' ? 'warning' : 'danger' }
function reservationCandidates(id: string) { return mappingWorkflow.value ? scopedMappingCandidates(mappingWorkflow.value, 'reservation', id) : [] }
function hardPayoffCandidates(id: string) { return mappingWorkflow.value ? scopedMappingCandidates(mappingWorkflow.value, 'payoff', id) : [] }
function userError(value: string) {
  if (value.includes('B14_ALREADY_COMPLETED')) return '当前版本已经完成完结确认。'
  if (value.includes('B14_COMPLETION_NOT_READY')) return '当前还有必须解决的问题，暂时不能提交完结。'
  if (value.includes('B14_REQUESTED_ENDING')) return '请先明确并确认本次结局目标。'
  if (value.includes('REVISION_CONFLICT')) return '作品状态刚刚发生变化，请重新检查后再提交。'
  return '暂时无法完成这项操作，请重新检查后重试。'
}

async function load() {
  if (!projectId.value) return
  loading.value = true
  error.value = ''
  try {
    status.value = await frontendPort.queryCompletionStatus({ projectId: projectId.value, scope: 'completion', pageSize: 1 })
    if (resultPending.value && committedReceiptId.value) {
      resultPending.value = false
      messageKind.value = 'success'
      message.value = '正式回执已返回，当前完结状态已重新读取。'
    }
    return true
  } catch (cause) {
    status.value = null
    error.value = resultPending.value ? '正式回执已返回，但重新查询未完成，结果待确认。请只重新读取，不要重复提交。' : cause instanceof Error ? userError(cause.message) : '完结状态暂时无法读取。'
    return false
  } finally {
    loading.value = false
  }
}

function commandId(prefix: string) {
  return `${prefix}:${projectId.value}:${Date.now()}`
}

async function submit(type: 'endgame_contract_create' | 'complete_book', payload: Record<string, unknown>, successMessage: string) {
  if (!status.value || submitting.value || resultPending.value) return
  const id = commandId(type)
  submitting.value = true
  error.value = ''
  message.value = ''
  messageKind.value = 'info'
  committedReceiptId.value = ''
  try {
    const receipt = await frontendPort.executeCommand({
      type,
      projectId: projectId.value,
      commandId: id,
      idempotencyKey: id,
      requestedAt: new Date().toISOString(),
      expectedRevision: status.value.projectRevision,
      payload,
    }, (_state: CommandStatus, detail: string) => { message.value = detail })
    if (receipt.commandId !== id || receipt.projectId !== projectId.value) {
      resultPending.value = true
      messageKind.value = 'warning'
      error.value = '操作回执与当前作品不匹配，结果待确认；请核对记录，不要重复提交。'
      return
    }
    if (receipt.finalStatus !== 'committed') {
      messageKind.value = 'warning'
      message.value = productErrorMessage(receipt.message, '本次操作未正式提交，请核对原因。')
      return
    }
    resultPending.value = true
    committedReceiptId.value = receipt.receiptId || receipt.commandId
    messageKind.value = 'warning'
    message.value = '正式回执已返回，正在重新读取结果。'
    if (await load()) {
      messageKind.value = 'success'
      message.value = successMessage
    }
  } catch (cause) {
    resultPending.value = true
    messageKind.value = 'warning'
    error.value = '尚未获得可确认的操作结果，请核对正式记录，不要重复提交。'
  } finally {
    submitting.value = false
  }
}

async function createContract() {
  if (!endingConfirmed.value || !summary.value.trim() || resultPending.value) return
  const workflow = mappingWorkflow.value
  await submit('endgame_contract_create', {
    requestedEnding: {
      summary: summary.value.trim(),
      confirmationSource: 'USER_CONFIRMED',
      confirmedAt: new Date().toISOString(),
    },
    mysteryMappings: (workflow?.mysteries ?? []).flatMap(item => mappingSelections.value[item.key] ? [{ sourceMysteryId: item.id, mappedResponsibilityRefs: [mappingSelections.value[item.key]], mappingConfirmation: 'USER_CONFIRMED' }] : []),
    reservationMappings: (workflow?.reservations ?? []).flatMap(item => mappingSelections.value[item.key] ? [{ reservationId: item.id, fulfillmentEvidenceRefs: [mappingSelections.value[item.key]], mappingConfirmation: 'USER_CONFIRMED' }] : []),
    hardPayoffMappings: (workflow?.hardPayoffs ?? []).flatMap(item => mappingSelections.value[item.key] ? [{ payoffId: item.id, fulfillmentEvidenceRefs: [mappingSelections.value[item.key]], fulfilled: true }] : [{ payoffId: item.id, fulfillmentEvidenceRefs: [], fulfilled: false }]),
  }, '终局目标与收束安排已保存，正在重新检查作品状态。')
}

async function loadMappingWorkflow() {
  if (!projectId.value) return
  mappingLoading.value = true
  try {
    mappingWorkflow.value = await frontendPort.queryEndgameMappingOptions({ projectId: projectId.value, scope: 'completion-mapping', pageSize: 30 })
  } catch (cause) {
    error.value = cause instanceof Error ? userError(cause.message) : '暂时无法读取可确认的收束依据。'
  } finally {
    mappingLoading.value = false
  }
}

async function completeBook() {
  if (!completionConfirmed.value || !canComplete.value) return
  await submit('complete_book', {}, '当前版本的完结状态与确认报告已经保存。')
}

async function requestEndgameAdvice() {
  if (!projectId.value) return
  endgameAdvice.value = await frontendPort.queryEndgameAdvice({ projectId: projectId.value, scope: 'completion-advice', pageSize: 1 })
}

async function requestQualityReview() {
  if (!projectId.value) return
  qualityReview.value = await frontendPort.queryCompletionQuality({ projectId: projectId.value, scope: 'completion-quality', pageSize: 1 })
}

onMounted(() => { void load() })
</script>

<template>
  <div class="page">
    <PageHeader title="完结准备" subtitle="依据项目正式记录检查终局目标、必须解决的问题与成品完整性；建议不替代正式阻断。">
      <button class="btn" :disabled="loading || submitting" @click="load">重新检查</button>
    </PageHeader>

    <div v-if="loading" class="card pad section" role="status">正在读取正式完结状态。</div>
    <div v-else-if="error" class="card pad section" role="alert">
      <h2 class="card-title">暂时无法完成检查</h2>
      <p class="card-subtitle">{{ error }}</p>
      <button class="btn" style="margin-top:16px" @click="load">再次检查</button>
    </div>
    <template v-else-if="status">
      <section class="card pad section">
        <div class="card-head">
          <div>
            <span class="badge" :class="statusClass">{{ statusText }}</span>
            <h2 style="margin:10px 0 4px">{{ status.completed ? '当前版本已完成完结确认' : '当前完结准备状态' }}</h2>
            <p class="card-subtitle">当前版本：{{ status.completed ? '已确认' : '需要继续检查' }}</p>
          </div>
          <strong class="completion-count">{{ status.blockerCount }} 个阻断</strong>
        </div>
      </section>

      <section v-if="!status.completed" class="card pad section">
        <h2 class="card-title">第一步：确认用户要求的结局</h2>
        <p class="card-subtitle">终局约束需要由你明确确认；未确认时不能进入完结提交。</p>
        <label class="field-label" style="margin-top:14px">你希望本书如何结束？<textarea v-model="summary" rows="4" placeholder="用一句或几句明确描述结局边界。" /></label>
        <label class="check-line"><input v-model="endingConfirmed" type="checkbox" />我确认以上内容是本次正式完结目标。</label>
        <button class="btn primary" :disabled="submitting || resultPending || !endingConfirmed || !summary.trim()" @click="createContract">确认终局目标并保存约束</button>
      </section>

      <section v-if="!status.completed" class="card pad section">
        <div class="card-head"><div><h2 class="card-title">需要确认的终局责任</h2><p class="card-subtitle">仅可选择当前作品已保存的责任与完成依据；不会自动建立对应关系。</p></div><button class="btn small" :disabled="mappingLoading || submitting" @click="loadMappingWorkflow">{{ mappingLoading ? '正在读取' : '查看可确认内容' }}</button></div>
        <template v-if="mappingWorkflow">
          <label v-for="item in mappingWorkflow.mysteries" :key="item.key" class="field-label" style="margin-top:12px">{{ item.label }}<select v-model="mappingSelections[item.key]"><option value="">暂不确认</option><option v-for="choice in mappingWorkflow.responsibilityCandidates" :key="choice.ref" :value="choice.ref">{{ choice.label }}</option></select></label>
          <template v-for="item in mappingWorkflow.reservations" :key="item.key">
            <label class="field-label" style="margin-top:12px">{{ item.label }}<select v-model="mappingSelections[item.key]"><option value="">暂不确认</option><option v-for="choice in reservationCandidates(item.id)" :key="choice.ref" :value="choice.ref">{{ choice.label }}</option></select></label>
            <p v-if="reservationCandidates(item.id).length === 0" class="muted" style="margin-top:6px">当前还没有可作为完成依据的正式记录。</p>
          </template>
          <template v-for="item in mappingWorkflow.hardPayoffs" :key="item.key">
            <label class="field-label" style="margin-top:12px">{{ item.label }}<select v-model="mappingSelections[item.key]"><option value="">尚未兑现</option><option v-for="choice in hardPayoffCandidates(item.id)" :key="choice.ref" :value="choice.ref">{{ choice.label }}</option></select></label>
            <p v-if="hardPayoffCandidates(item.id).length === 0" class="muted" style="margin-top:6px">当前还没有可作为完成依据的正式记录。</p>
          </template>
          <p v-if="!mappingWorkflow.mysteries.length && !mappingWorkflow.reservations.length && !mappingWorkflow.hardPayoffs.length" class="muted" style="margin-top:12px">当前没有需要确认的谜团、未来约束或关键回报。</p>
        </template>
      </section>

      <section class="card pad section">
        <div class="card-head"><div><h2 class="card-title">创作建议</h2><p class="card-subtitle">只提供辅助意见，不会改变正式完结结论。</p></div><div class="actions"><button class="btn small" :disabled="submitting" @click="requestEndgameAdvice">生成结局建议</button><button class="btn small" :disabled="submitting" @click="requestQualityReview">检查回报质量</button></div></div>
        <div v-if="endgameAdvice" class="banner info" style="margin-top:14px"><div><div class="banner-title">结局建议</div><div class="banner-copy">{{ endgameAdvice.proposal?.summary }}<br /><span v-for="item in endgameAdvice.proposal?.suggestions ?? []" :key="item">{{ item }}<br /></span></div></div></div>
        <div v-if="qualityReview" class="banner info" style="margin-top:10px"><div><div class="banner-title">回报质量建议</div><div class="banner-copy">{{ qualityReview.review?.summary }}<br /><span v-for="item in qualityReview.review?.suggestions ?? []" :key="item">{{ item }}<br /></span></div></div></div>
      </section>

      <section class="section">
        <div class="section-head"><div><h2>六项正式检查</h2><div class="muted" style="font-size:12px">只展示可追溯的正式来源；软建议不会单独阻断完结。</div></div></div>
        <div class="workspace-cards">
          <article v-for="check in status.checks" :key="check.kind" class="card pad">
            <div class="card-head"><h3 style="margin:0">{{ checkName(check.kind) }}</h3><span class="badge" :class="checkClass(check.status)">{{ checkStatus(check.status) }}</span></div>
            <p class="card-subtitle" style="margin-top:10px">{{ check.summary }}</p>
            <div class="muted" style="font-size:12px">阻断 {{ check.blockerCount }} · 建议 {{ check.advisoryCount }}</div>
            <div class="muted" style="font-size:11px;margin-top:8px">来源：当前作品正式记录</div>
          </article>
        </div>
      </section>

      <section v-if="!status.completed" class="card pad section">
        <h2 class="card-title">第二步：提交全书完结</h2>
        <p class="card-subtitle">只有六项检查全部满足时，系统才会一次性保存完结状态与确认记录。</p>
        <label class="check-line"><input v-model="completionConfirmed" type="checkbox" />我确认现在提交全书完结。</label>
        <button class="btn primary" :disabled="submitting || !canComplete || !completionConfirmed" @click="completeBook">提交全书完结</button>
      </section>

      <div v-if="message" class="banner section" :class="messageKind" role="status"><div class="banner-title">{{ messageKind === 'success' ? '已确认' : messageKind === 'warning' ? '需要核对' : '正在处理' }}</div><div class="banner-copy">{{ message }}</div></div>
    </template>
  </div>
</template>

<style scoped>
.completion-count { color: var(--ink-2); font-size: 14px; white-space: nowrap; }
.field-label { display: grid; gap: 7px; color: var(--ink-2); font-size: 13px; }
.field-label textarea { resize: vertical; width: 100%; box-sizing: border-box; min-height: 92px; border: 1px solid var(--line); border-radius: 8px; padding: 10px; background: var(--surface); color: var(--ink); font: inherit; }
.check-line { display: flex; gap: 8px; align-items: center; margin: 14px 0; color: var(--ink-2); font-size: 13px; }
</style>
