<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { presentAssistantSession } from '../presentation/assistantPresentation'
import type { AssistantPresentation } from '../presentation/assistantPresentation'

defineProps<{ collapsed: boolean }>()
const emit = defineEmits<{ toggle: [] }>()
const route = useRoute()
const router = useRouter()
const store = useProjectStore()
const question = ref('')
const lastQuestion = ref('')
const sending = ref(false)
const evidenceOpen = ref(false)
const presentation = ref<AssistantPresentation | null>(null)
const context = computed(() => ({ 'project-overview': '总览', production: '创作', directions: '规划', volumes: '当前卷', 'volume-workspace': '当前卷', decisions: '待处理', health: '长篇健康', read: '资料', graph: '资料', completion: '完结准备' }[String(route.name)] ?? '当前作品'))
const placeholder = computed(() => context.value === '当前卷' ? '问问这一卷还缺什么、为什么卡住……' : context.value === '创作' ? '问问这一章的设定、冲突或修改建议……' : '问问这部作品的进展、问题或下一步……')

async function send() {
  const text = question.value.trim()
  if (!text || sending.value) return
  sending.value = true
  lastQuestion.value = text
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
  <aside class="prototype-assistant" :class="{ collapsed }" aria-label="项目助手">
    <div class="prototype-assistanthead">
      <div class="prototype-logo" style="width:30px;height:30px;border-radius:9px">墨</div>
      <div class="prototype-atitle"><strong>项目助手</strong><small>正在查看：{{ context }}</small></div>
      <div class="prototype-assistanttools"><button class="prototype-iconbtn" type="button" title="新对话" @click="presentation = null">＋</button></div>
      <button class="prototype-iconbtn" type="button" :title="collapsed ? '展开助手' : '折叠助手'" @click="emit('toggle')">{{ collapsed ? '‹' : '›' }}</button>
    </div>

    <div class="prototype-messages">
      <template v-if="presentation">
        <div class="prototype-msg user">{{ lastQuestion }}</div>
        <div class="prototype-msg assistantmsg"><div class="prototype-answercard"><h4>{{ presentation.status === 'supported' ? '已完成当前记录核对' : '还需要继续核对' }}</h4><p>{{ presentation.answer }}</p><div v-if="presentation.evidence.length" class="prototype-evidence"><span>依据来自当前作品记录</span><button class="prototype-link" type="button" @click="evidenceOpen = !evidenceOpen">{{ evidenceOpen ? '收起依据' : '查看依据' }}</button></div><div v-if="evidenceOpen" class="prototype-proposal"><strong>已核对的依据</strong><p v-for="item in presentation.evidence" :key="`${item.label}-${item.source ?? ''}`">{{ item.label }} · {{ item.detail }}</p></div><div class="prototype-proposal"><strong>需要修改时</strong><p>我可以整理一份修改方案，但不会直接改动已经确认的故事事实。</p><button class="prototype-btn" type="button" @click="router.push(`/projects/${store.currentProjectId}/assistant`)">查看修改方案</button></div></div></div>
      </template>
      <div v-else class="prototype-msg assistantmsg"><div class="prototype-answercard"><h4>从一件具体的事开始</h4><p>我会先核对当前作品记录，再告诉你能够确认到什么。涉及正式修改时，会先给出方案并等待你的确认。</p><div class="prototype-evidence"><span>当前上下文：{{ context }}</span><span>等待提问</span></div></div></div>
    </div>

    <div class="prototype-composer"><div class="prototype-inputbox"><textarea v-model="question" :placeholder="placeholder" :disabled="sending" @keydown.enter.exact.prevent="send" /><div class="prototype-composerbar"><span class="prototype-hint">Shift + Enter 换行</span><button class="prototype-btn primary" type="button" :disabled="!question.trim() || sending" @click="send">{{ sending ? '核对中…' : '发送' }}</button></div></div></div>
  </aside>
</template>
