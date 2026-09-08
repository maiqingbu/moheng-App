<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

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
async function submit() {
  if (!selected.value) return
  await store.runCommand(commandKey.value, {
    type: 'decision_submit',
    payload: { decisionId: selected.value.id, choice: 'recommended', note: note.value }
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
        <div v-if="!items.length" class="empty">当前没有符合条件的决定。</div>
      </section>

      <section v-if="selected" class="card decision-detail">
        <span class="badge">{{ selected.category }}</span>
        <h2>{{ selected.title }}</h2>
        <p>{{ selected.summary }}</p>
        <div v-if="selected.recommendation" class="banner info">
          <div><div class="banner-title">系统建议</div><div class="banner-copy">{{ selected.recommendation }}</div></div>
        </div>
        <label class="field section"><span class="field-label">补充意见</span><textarea v-model="note" class="textarea" placeholder="用读者语言描述你的要求" /></label>
        <CommandFeedback :state="state" />
        <button v-if="selected.status === 'pending'" class="btn primary" @click="submit">采用建议并继续</button>
      </section>
    </div>
  </div>
</template>
