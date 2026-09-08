<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { presentAssistantSession } from '../presentation/assistantPresentation'
import type { AssistantPresentation } from '../presentation/assistantPresentation'

defineProps<{ collapsed: boolean }>()
const emit = defineEmits<{ toggle: [] }>()
const store = useProjectStore()
const route = useRoute()
const question = ref('')
const sending = ref(false)
const evidenceOpen = ref(false)
const presentation = ref<AssistantPresentation | null>(null)
const contextLabel = computed(() => {
  const labels: Record<string, string> = { 'project-overview': '总览', production: '创作', directions: '规划', volumes: '当前卷', 'volume-workspace': '当前卷', decisions: '待处理', health: '长篇健康', read: '资料', graph: '资料', completion: '完结准备' }
  return labels[String(route.name)] ?? '当前作品'
})
const placeholder = computed(() => ({ 总览: '问问这部作品的进展、问题或下一步……', 创作: '问问这一章的设定、冲突或修改建议……', 规划: '问问全书或当前卷的规划依据……', 当前卷: '问问这一卷还缺什么、为什么卡住……', 待处理: '问问哪些事情需要你确认……', '长篇健康': '问问长篇状态和需要关注的地方……', 资料: '问问人物、事件、线索或设定……', 完结准备: '问问当前完结准备还缺什么……' }[contextLabel.value] ?? '问问这部作品……'))

async function send() {
  const text = question.value.trim()
  if (!text || sending.value) return
  sending.value = true
  try {
    const session = await store.supportSession(text, 'DIAGNOSE')
    if (session) presentation.value = presentAssistantSession(session)
    question.value = ''
  } finally {
    sending.value = false
  }
}
</script>

<template>
  <aside class="assistant-dock" :class="{ collapsed }" aria-label="项目助手">
    <template v-if="collapsed">
      <button class="assistant-rail-button" type="button" aria-label="展开项目助手" @click="emit('toggle')"><span class="assistant-mark">墨</span><span class="assistant-rail-label">助手</span></button>
    </template>
    <template v-else>
      <header class="assistant-head">
        <span class="assistant-mark">墨</span>
        <div class="assistant-title"><strong>项目助手</strong><span>{{ store.project?.title ?? '当前作品' }}</span></div>
        <button class="icon-btn" type="button" aria-label="折叠项目助手" @click="emit('toggle')">›</button>
      </header>
      <div class="assistant-context"><span class="dot" />当前上下文：{{ contextLabel }}</div>
      <div class="assistant-messages">
        <div v-if="!presentation" class="assistant-empty"><div class="assistant-empty-mark">问</div><strong>从一件具体的事开始</strong><p>我会先核对当前作品记录，再告诉你能确认到什么。涉及正式修改时，会先给出方案并等待你的确认。</p></div>
        <div v-else class="assistant-answer">
          <div class="assistant-answer-head"><span>查询结果</span><span class="badge" :class="presentation.status === 'supported' ? 'success' : 'warning'">{{ presentation.status === 'supported' ? '有记录支持' : '需要继续核对' }}</span></div>
          <p class="assistant-answer-text">{{ presentation.answer }}</p>
          <div class="assistant-sections"><div v-for="section in presentation.sections.slice(0, 3)" :key="section.title" class="assistant-section"><strong>{{ section.title }}</strong><span>{{ section.body }}</span></div></div>
          <button v-if="presentation.evidence.length" type="button" class="assistant-evidence-toggle" @click="evidenceOpen = !evidenceOpen">{{ evidenceOpen ? '收起依据' : `查看依据（${presentation.evidence.length}）` }}</button>
          <div v-if="evidenceOpen" class="assistant-evidence-list"><div v-for="item in presentation.evidence" :key="`${item.label}-${item.source ?? ''}`"><strong>{{ item.label }}</strong><span>{{ item.detail }}</span></div></div>
          <RouterLink :to="`/projects/${store.currentProjectId}/assistant`" class="assistant-detail-link">打开详细处理</RouterLink>
        </div>
      </div>
      <div class="assistant-composer">
        <textarea v-model="question" :placeholder="placeholder" :disabled="sending" @keydown.enter.exact.prevent="send" />
        <div class="assistant-composer-foot"><span>Enter 发送 · Shift+Enter 换行</span><button class="btn primary small" type="button" :disabled="!question.trim() || sending" @click="send">{{ sending ? '核对中…' : '发送' }}</button></div>
      </div>
    </template>
  </aside>
</template>
