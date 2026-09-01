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
const historyStatusLabel = (value: string) => ({ applied: '已生效', approved: '已确认', impact_ready: '影响已算出', draft: '草稿', pending: '待确认' } as Record<string, string>)[value] ?? '待确认'

onMounted(() => void store.loadHistory())
async function setKind(next: HistoryKind) { kind.value = next; await store.loadHistory(next, true) }
async function create() {
  const type = modal.value === 'retcon' ? 'retcon_plan_create' : 'change_request_create'
  await store.runCommand('history-create', { type, payload: { request: requestText.value } })
  modal.value = null
  requestText.value = ''
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

    <BaseModal :open="!!modal" :title="modal === 'retcon' ? '建立事实修订计划' : '建立修改请求'" :subtitle="modal === 'retcon' ? '重大正式事实不会被普通保存覆盖。' : '先计算影响，再决定是否应用。'" @close="modal = null">
      <textarea v-model="requestText" class="textarea" placeholder="描述你希望改变的内容" />
      <CommandFeedback :state="state" />
      <template #footer><button class="btn" @click="modal = null">取消</button><button class="btn primary" :disabled="!requestText.trim()" @click="create">开始影响分析</button></template>
    </BaseModal>
  </div>
</template>
