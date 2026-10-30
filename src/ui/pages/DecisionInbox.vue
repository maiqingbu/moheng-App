<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { RouterLink } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import LoadingBlock from '../components/LoadingBlock.vue'
import { decisionWorkspaces } from '../presentation/decisionWorkspaces'
import { isInFlight } from '../presentation/productLanguage'

const store = useProjectStore()
type DecisionFilter = 'pending' | 'resolved' | 'all'
const filters: { value: DecisionFilter; label: string }[] = [
  { value: 'pending', label: '待处理' },
  { value: 'resolved', label: '已处理' },
  { value: 'all', label: '全部' }
]
const filter = ref<DecisionFilter>('pending')
const selectedId = ref('')
const note = ref('')
const items = computed(() => store.decisions?.items ?? [])
const selected = computed(() => items.value.find(item => item.id === selectedId.value) ?? items.value[0])
const commandKey = computed(() => `decision-${selected.value?.id ?? 'none'}`)
const state = computed(() => store.commandState(commandKey.value))

async function load() {
  await store.loadDecisions(filter.value, true)
  selectedId.value = items.value[0]?.id ?? ''
}
function setFilter(next: DecisionFilter) { filter.value = next }
const actionLabels = {
  RETRY_REMOTE: '重新尝试',
  MARK_FAILED: '标记失败',
  CANCEL_JOB: '取消任务',
  ACKNOWLEDGE: '知道了'
} as const
const allowedActions = computed(() => selected.value?.allowedActions ?? [])
// Only actions on a remote job take a note; closing a reminder does not.
const takesNote = computed(() => allowedActions.value.some(action => action !== 'ACKNOWLEDGE'))
const inFlight = computed(() => isInFlight(state.value.status))
const workspace = computed(() => selected.value ? decisionWorkspaces[selected.value.category] : undefined)
async function submit(choice: keyof typeof actionLabels) {
  if (!selected.value) return
  await store.runCommand(commandKey.value, {
    type: 'decision_submit',
    payload: { decisionId: selected.value.id, choice, note: note.value }
  })
  note.value = ''
  await load()
}

onMounted(load)
watch(filter, load)
</script>

<template>
  <div class="page">
    <PageHeader title="待处理" subtitle="把需要确认、修改、重新检查或授权的事情集中在这里处理。" />

    <div class="segmented" style="width:max-content;margin-bottom:14px">
      <button v-for="item in filters" :key="item.value" :class="{ active: filter === item.value }" @click="setFilter(item.value)">{{ item.label }}</button>
    </div>

    <div class="decision-layout">
      <section class="card decision-list">
        <button v-for="decision in items" :key="decision.id" class="decision-item" :class="{ active: selected?.id === decision.id }" @click="selectedId = decision.id">
          <span class="badge" :class="decision.priority === 'blocking' ? 'danger' : decision.priority === 'high' ? 'warning' : ''">{{ decision.category }}</span>
          <h3>{{ decision.title }}</h3>
          <div class="muted" style="font-size:11px">{{ decision.summary }}</div>
        </button>
        <LoadingBlock v-if="!store.decisions" caption="正在读取决定…" />
        <div v-else-if="!items.length" class="empty">当前没有符合条件的决定。</div>
      </section>

      <section v-if="selected" class="card decision-detail">
        <span class="badge">{{ selected.category }}</span>
        <h2>{{ selected.title }}</h2>
        <p>{{ selected.summary }}</p>
        <div v-if="selected.recommendation" class="banner info">
          <div><div class="banner-title">系统建议</div><div class="banner-copy">{{ selected.recommendation }}</div></div>
        </div>
        <label v-if="selected.status === 'pending' && takesNote" class="field section"><span class="field-label">补充意见</span><textarea v-model="note" class="textarea" placeholder="用读者语言描述你的要求" /></label>
        <CommandFeedback :state="state" />
        <div v-if="selected.status === 'pending' && allowedActions.length" class="actions">
          <button v-for="action in allowedActions" :key="action" class="btn" :class="{ primary: action === 'RETRY_REMOTE' || action === 'ACKNOWLEDGE' }" :disabled="inFlight" :aria-busy="inFlight" @click="submit(action)">{{ actionLabels[action] }}</button>
          <RouterLink v-if="workspace && store.project" class="btn" :to="`/projects/${store.project.id}/${workspace.path}`">{{ workspace.label }}</RouterLink>
        </div>
        <div v-else-if="selected.status === 'pending'" class="actions section">
          <RouterLink v-if="workspace && store.project" class="btn primary" :to="`/projects/${store.project.id}/${workspace.path}`">{{ workspace.label }}</RouterLink>
          <span class="muted">这项决定在对应工作区完成。</span>
        </div>
      </section>
    </div>
  </div>
</template>
