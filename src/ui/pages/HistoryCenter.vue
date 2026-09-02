<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import BaseModal from '../components/BaseModal.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
type HistoryKind = 'all' | 'VERSION' | 'CHANGE_REQUEST' | 'RETCON'
const filters: { value: HistoryKind; label: string }[] = [
  { value: 'all', label: '全部' },
  { value: 'VERSION', label: '版本' },
  { value: 'CHANGE_REQUEST', label: '修改' },
  { value: 'RETCON', label: '事实修订' }
]
const kind = ref<HistoryKind>('all')
const requestText = ref('')
const modal = ref<'change' | 'retcon' | null>(null)
const history = computed(() => store.history)
const state = computed(() => store.commandState('history-create'))
const impactState = computed(() => store.commandState('history-impact'))
const decisionState = computed(() => store.commandState('history-decision'))
const applyState = computed(() => store.commandState('history-apply'))
const activeChange = ref<{ id: string; type: 'CHANGE' | 'RETCON'; digest: string; scope: string; status: string; expandedEdges: number; totalEdges: number } | null>(null)
const impactItems = ref<string[]>([])
const actionMessage = ref('')
const historyStatusLabel = (value: string) => ({ applied: '已生效', approved: '已确认', impact_ready: '影响已算出', draft: '草稿', pending: '待确认' } as Record<string, string>)[value] ?? '待确认'

onMounted(() => void store.loadHistory())
async function setKind(next: HistoryKind) { kind.value = next; await store.loadHistory(next, true) }
async function create() {
  const projectId = store.currentProjectId
  if (!projectId || !requestText.value.trim()) return
  const type = modal.value === 'retcon' ? 'RETCON' : 'CHANGE'
  const changeId = `change:${projectId}:${Date.now()}`
  const created = await store.runCommand('history-create', {
    type: 'change_request_create',
    projectId,
    payload: {
      changeRequestId: changeId,
      changeType: type,
      scope: 'PROJECT',
      summary: requestText.value.trim(),
      reason: requestText.value.trim(),
      requestedChange: { description: requestText.value.trim() },
    },
  })
  const preflight = await store.runCommand('history-impact', {
    type: 'impact_preflight',
    projectId,
    payload: { changeRequestId: changeId },
  })
  const result = preflight.result ?? created.result ?? {}
  activeChange.value = {
    id: changeId,
    type,
    digest: typeof result.impactDigest === 'string' ? result.impactDigest : '',
    scope: typeof result.scopeClass === 'string' ? result.scopeClass : 'LOCAL',
    status: typeof result.status === 'string' ? result.status : 'PREFLIGHTED',
    expandedEdges: typeof result.expandedEdges === 'number' ? result.expandedEdges : 0,
    totalEdges: typeof result.totalEdges === 'number' ? result.totalEdges : 0,
  }
  impactItems.value = []
  modal.value = null
  requestText.value = ''
  await store.loadHistory('all', true)
}
async function continueImpact() {
  if (!activeChange.value) return
  const result = await store.runCommand('history-impact', {
    type: 'impact_expand',
    projectId: store.currentProjectId ?? undefined,
    payload: { changeRequestId: activeChange.value.id, pageSize: 40, continueGlobal: true },
  })
  const next = result.result ?? {}
  activeChange.value = { ...activeChange.value, status: String(next.status ?? activeChange.value.status), expandedEdges: Number(next.expandedEdges ?? activeChange.value.expandedEdges), totalEdges: Number(next.totalEdges ?? activeChange.value.totalEdges), digest: String(next.impactDigest ?? activeChange.value.digest) }
  const items = Array.isArray(next.items) ? next.items : []
  impactItems.value = [...impactItems.value, ...items.map(item => {
    const target = item && typeof item === 'object' && typeof (item as Record<string, unknown>).targetRef === 'string' ? (item as Record<string, unknown>).targetRef as string : ''
    const chapter = target.match(/(?:chapter:|第)(?:[^:：]*[:：])?(\d+)$/i)?.[1]
    return chapter ? `第 ${chapter} 章` : '相关规划内容'
  })]
}
async function cancelImpact() {
  if (!activeChange.value) return
  await store.runCommand('history-impact', { type: 'impact_cancel', projectId: store.currentProjectId ?? undefined, payload: { changeRequestId: activeChange.value.id } })
  activeChange.value = { ...activeChange.value, status: 'CANCELLED' }
}
async function rejectChange() {
  if (!activeChange.value || !activeChange.value.digest) return
  await store.runCommand('history-decision', { type: 'change_request_decide', projectId: store.currentProjectId ?? undefined, payload: { changeRequestId: activeChange.value.id, decision: 'REJECT', impactDigest: activeChange.value.digest, scopeAcknowledged: true, riskAcknowledged: true } })
  activeChange.value = { ...activeChange.value, status: 'REJECTED' }
}
async function approveAndApply() {
  if (!activeChange.value || !activeChange.value.digest) return
  if (activeChange.value.type === 'RETCON') {
    actionMessage.value = '这项请求已识别为事实修订，需要先补充事实修订方案和依据。'
    return
  }
  await store.runCommand('history-decision', {
    type: 'change_request_decide',
    projectId: store.currentProjectId ?? undefined,
    payload: { changeRequestId: activeChange.value.id, decision: 'APPROVE', impactDigest: activeChange.value.digest, scopeAcknowledged: true, riskAcknowledged: true },
  })
  const result = await store.runCommand('history-apply', {
    type: 'change_request_apply',
    projectId: store.currentProjectId ?? undefined,
    payload: { changeRequestId: activeChange.value.id, impactDigest: activeChange.value.digest },
  })
  activeChange.value = { ...activeChange.value, status: result.postverify?.status === 'VERIFIED' ? 'APPLIED' : 'VERIFY_REQUIRED' }
  await store.loadHistory('all', true)
}
</script>

<template>
  <div v-if="history" class="page">
    <PageHeader title="版本与变更记录" subtitle="正式历史不可覆盖；普通修改先做影响分析，已生效的核心事实需要单独确认。">
      <button class="btn" @click="modal = 'change'">新建修改</button>
      <button class="btn warning" @click="modal = 'retcon'">新建事实修订计划</button>
    </PageHeader>

    <div class="metric-grid section">
      <div class="metric-card"><div class="metric-label">版本记录</div><div class="metric-value">{{ history.stats.versions }}</div></div>
      <div class="metric-card"><div class="metric-label">修改请求</div><div class="metric-value">{{ history.stats.changes }}</div></div>
      <div class="metric-card"><div class="metric-label">事实修订</div><div class="metric-value">{{ history.stats.retcons }}</div></div>
      <div class="metric-card"><div class="metric-label">待确认</div><div class="metric-value">{{ history.stats.pending }}</div></div>
    </div>

    <div class="segmented section" style="width:max-content">
      <button v-for="item in filters" :key="item.value" :class="{ active: kind === item.value }" @click="setKind(item.value)">{{ item.label }}</button>
    </div>

    <section v-if="activeChange" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">当前变更方案</h2><p class="card-subtitle">先看预计影响，再决定是否继续；正式执行后会再次核对结果。</p></div><span class="badge" :class="activeChange.status === 'APPLIED' ? 'success' : 'warning'">{{ activeChange.status === 'APPLIED' ? '已完成并复核' : activeChange.status === 'CANCELLED' ? '已取消' : '等待确认' }}</span></div>
      <div class="metric-grid"><div class="metric-card"><div class="metric-label">影响范围</div><div class="metric-value">{{ activeChange.scope === 'GLOBAL' ? '全局' : activeChange.scope === 'WIDE' ? '较广' : '局部' }}</div></div><div class="metric-card"><div class="metric-label">已分析内容</div><div class="metric-value">{{ activeChange.expandedEdges }} / {{ activeChange.totalEdges }}</div></div></div>
      <div v-if="impactItems.length" class="banner info"><div><div class="banner-title">受影响内容</div><div class="banner-copy">{{ [...new Set(impactItems)].slice(0, 12).join('、') }}{{ impactItems.length > 12 ? ' 等' : '' }}</div></div></div>
      <p v-if="activeChange.type === 'RETCON'" class="banner warning">这项请求涉及已经确认的事实，需先查看原事实、依据和事实修订方案。</p>
      <p v-if="actionMessage" class="banner warning">{{ actionMessage }}</p>
      <div class="actions" style="justify-content:flex-end;margin-top:14px"><button class="btn small" :disabled="activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED'" @click="continueImpact">继续分析影响</button><button class="btn small" :disabled="activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED'" @click="cancelImpact">取消分析</button><button class="btn small" :disabled="!activeChange.digest || activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED' || activeChange.expandedEdges < activeChange.totalEdges" @click="rejectChange">拒绝方案</button><button class="btn primary small" :disabled="activeChange.type === 'RETCON' || !activeChange.digest || activeChange.status === 'APPLIED' || activeChange.status === 'CANCELLED' || activeChange.status === 'REJECTED' || activeChange.expandedEdges < activeChange.totalEdges" @click="approveAndApply">确认并执行</button></div>
      <CommandFeedback :state="impactState" /><CommandFeedback :state="decisionState" /><CommandFeedback :state="applyState" />
    </section>

    <div style="display:grid;gap:10px;margin-top:14px">
      <article v-for="entry in history.entries.items" :key="entry.id" class="card pad">
        <div class="card-head">
          <div>
            <span class="badge">{{ entry.kind === 'VERSION' ? '版本' : entry.kind === 'CHANGE_REQUEST' ? '修改请求' : '事实修订' }}</span>
            <h3 style="margin:8px 0 4px">{{ entry.title }}</h3>
            <p class="muted">{{ entry.summary }}</p>
          </div>
          <span class="badge" :class="entry.status === 'applied' ? 'success' : entry.status === 'impact_ready' ? 'warning' : ''">{{ historyStatusLabel(entry.status) }}</span>
        </div>
        <div class="banner info"><div><div class="banner-title">影响范围 · {{ entry.scope }}</div><div class="banner-copy">{{ entry.impact }}</div></div></div>
      </article>
    </div>

    <BaseModal :open="!!modal" :title="modal === 'retcon' ? '建立事实修订提案' : '建立修改请求'" :subtitle="modal === 'retcon' ? '重大正式事实不会被普通保存覆盖，需补充事实修订方案。' : '先计算影响，再决定是否应用。'" @close="modal = null">
      <textarea v-model="requestText" class="textarea" placeholder="描述你希望改变的内容" />
      <CommandFeedback :state="state" />
      <template #footer><button class="btn" @click="modal = null">取消</button><button class="btn primary" :disabled="!requestText.trim()" @click="create">开始影响分析</button></template>
    </BaseModal>
  </div>
</template>
