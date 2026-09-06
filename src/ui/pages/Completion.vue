<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import { getFrontendPort } from '../../app/providers/runtimePort'
import type { CommandStatus, CompletionStatusProjection, SoftCreativeAdviceProjection } from '../../application/contracts'

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
const endgameAdvice = ref<SoftCreativeAdviceProjection | null>(null)
const qualityReview = ref<SoftCreativeAdviceProjection | null>(null)

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
const canComplete = computed(() => status.value?.status === 'READY' || (status.value?.status === 'NEEDS_RECHECK' && status.value.blockerCount === 0 && status.value.checks.every(check => check.status === 'READY')))

function checkName(kind: string) {
  return ({ OBLIGATION: '正式剧情责任', MYSTERY: '重要谜团', PAYOFF: '关键回报', REQUESTED_ENDING: '用户确认的结局', CHANGE_STALE: '变更与规划', EXPORT_INTEGRITY: '成品完整性' } as Record<string, string>)[kind] ?? '完结条件'
}
function checkStatus(status: string) {
  return ({ READY: '已满足', BLOCKED: '还有必须解决的问题', NEEDS_DECISION: '需要先确认', NEEDS_RECHECK: '需要重新检查', SOURCE_UNAVAILABLE: '暂时无法完成检查' } as Record<string, string>)[status] ?? '需要重新检查'
}
function checkClass(status: string) { return status === 'READY' ? 'success' : status === 'NEEDS_DECISION' || status === 'NEEDS_RECHECK' ? 'warning' : 'danger' }
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
  } catch (cause) {
    error.value = cause instanceof Error ? userError(cause.message) : '完结状态暂时无法读取。'
  } finally {
    loading.value = false
  }
}

function commandId(prefix: string) {
  return `${prefix}:${projectId.value}:${Date.now()}`
}

async function submit(type: 'endgame_contract_create' | 'complete_book', payload: Record<string, unknown>, successMessage: string) {
  if (!status.value) return
  submitting.value = true
  error.value = ''
  message.value = ''
  try {
    await frontendPort.executeCommand({
      type,
      projectId: projectId.value,
      commandId: commandId(type),
      idempotencyKey: commandId(`idempotency:${type}`),
      requestedAt: new Date().toISOString(),
      expectedRevision: status.value.projectRevision,
      payload,
    }, (_state: CommandStatus, detail: string) => { message.value = detail })
    message.value = successMessage
    await load()
  } catch (cause) {
    error.value = cause instanceof Error ? userError(cause.message) : '提交失败，请检查正式状态后重试。'
  } finally {
    submitting.value = false
  }
}

async function createContract() {
  if (!endingConfirmed.value || !summary.value.trim()) return
  await submit('endgame_contract_create', {
    requestedEnding: {
      summary: summary.value.trim(),
      confirmationSource: 'USER_CONFIRMED',
      confirmedAt: new Date().toISOString(),
    },
  }, '终局目标已提交，正在重新检查正式状态。')
}

async function completeBook() {
  if (!completionConfirmed.value || !canComplete.value) return
  await submit('complete_book', {}, '完结提交已完成，ExitState 与 CompletionReport 已写入。')
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
    <PageHeader title="终局与完结" subtitle="只依据项目正式记录检查全书是否具备完结条件；建议不替代硬性阻断。">
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
        <p class="card-subtitle">终局合同必须由用户明确确认；未确认时不能进入完结提交。</p>
        <label class="field-label" style="margin-top:14px">你希望本书如何结束？<textarea v-model="summary" rows="4" placeholder="用一句或几句明确描述结局边界。" /></label>
        <label class="check-line"><input v-model="endingConfirmed" type="checkbox" />我确认以上内容是本次正式完结目标。</label>
        <button class="btn primary" :disabled="submitting || !endingConfirmed || !summary.trim()" @click="createContract">确认终局目标并创建合同</button>
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
        <p class="card-subtitle">只有六项检查全部 READY 时，系统才会在一个正式事务中写入 BookExitState、BookCompletionReport 与回执。</p>
        <label class="check-line"><input v-model="completionConfirmed" type="checkbox" />我确认现在提交全书完结。</label>
        <button class="btn primary" :disabled="submitting || !canComplete || !completionConfirmed" @click="completeBook">提交全书完结</button>
      </section>

      <div v-if="message" class="banner success section" role="status"><div class="banner-title">已处理</div><div class="banner-copy">{{ message }}</div></div>
    </template>
  </div>
</template>

<style scoped>
.completion-count { color: var(--ink-2); font-size: 14px; white-space: nowrap; }
.field-label { display: grid; gap: 7px; color: var(--ink-2); font-size: 13px; }
.field-label textarea { resize: vertical; width: 100%; box-sizing: border-box; min-height: 92px; border: 1px solid var(--line); border-radius: 8px; padding: 10px; background: var(--surface); color: var(--ink); font: inherit; }
.check-line { display: flex; gap: 8px; align-items: center; margin: 14px 0; color: var(--ink-2); font-size: 13px; }
</style>
