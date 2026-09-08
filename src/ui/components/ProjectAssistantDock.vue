<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useProjectStore } from '../stores/projectStore'
import { presentAssistantSession } from '../presentation/assistantPresentation'
import { productErrorMessage, sanitizeProductText } from '../presentation/productLanguage'
import type { AssistantPresentation } from '../presentation/assistantPresentation'
import type { AssistantRepairPlan } from '../../application/contracts'

type AssistantPhase = 'PROPOSAL_READY' | 'AWAITING_AUTHORIZATION' | 'EXECUTING' | 'EXECUTED' | 'POST_VERIFYING' | 'VERIFIED' | 'FAILED'

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
const proposal = ref<AssistantRepairPlan | null>(null)
const proposalOpen = ref(false)
const proposalMessage = ref('')
const assistantError = ref('')
const assistantPhase = ref<AssistantPhase>('PROPOSAL_READY')
const executionBeforeRevision = ref<number | null>(null)
const context = computed(() => ({ 'project-overview': '总览', production: '创作', directions: '规划', volumes: '当前卷', 'volume-workspace': '当前卷', decisions: '待处理', health: '长篇健康', read: '资料', graph: '资料', completion: '完结准备' }[String(route.name)] ?? '当前作品'))
const projectTitle = computed(() => store.project?.title?.trim() || '当前作品')
const placeholder = computed(() => context.value === '当前卷' ? '问问这一卷还缺什么、为什么卡住……' : context.value === '创作' ? '问问这一章的设定、冲突或修改建议……' : '问问这部作品的进展、问题或下一步……')
const proposalState = computed(() => store.commandState('assistant-plan'))
const authorizationState = computed(() => store.commandState('assistant-authorize'))
const executionState = computed(() => store.commandState('assistant-execute'))
const displayScopes = (values: unknown) => Array.isArray(values)
  ? values.map(value => sanitizeProductText(value, '当前作品')).filter(Boolean).join('、') || '当前作品'
  : '当前作品'

async function send() {
  const text = question.value.trim()
  if (!text || sending.value) return
  sending.value = true
  lastQuestion.value = text
  try {
    const session = await store.supportSession(text, 'DIAGNOSE')
    if (session) presentation.value = presentAssistantSession(session)
    question.value = ''
    assistantError.value = ''
  } catch (error) {
    assistantError.value = productErrorMessage(error, '暂时无法核对当前作品，请稍后重试。')
  } finally {
    sending.value = false
  }
}

async function prepareProposal() {
  const text = lastQuestion.value || question.value.trim()
  if (!text || proposalState.value.status === 'submitting') return
  proposalMessage.value = ''
  assistantError.value = ''
  try {
    proposal.value = await store.prepareAssistantRepair(text)
    assistantPhase.value = proposal.value.action === 'EXECUTABLE' ? 'AWAITING_AUTHORIZATION' : 'PROPOSAL_READY'
    proposalOpen.value = true
  } catch (error) {
    proposal.value = null
    proposalOpen.value = true
    assistantPhase.value = 'FAILED'
    proposalMessage.value = productErrorMessage(error, '暂时无法核对修改范围，请刷新当前作品后重试。')
  }
}

async function authorizeProposal() {
  if (assistantPhase.value !== 'AWAITING_AUTHORIZATION' || !proposal.value || proposal.value.action !== 'EXECUTABLE' || authorizationState.value.status === 'submitting') return
  executionBeforeRevision.value = store.project?.revision ?? null
  assistantPhase.value = 'EXECUTING'
  proposalMessage.value = '正在执行修改。'
  try {
    const receipt = await store.authorizeAssistantRepair(proposal.value)
    if (receipt.finalStatus !== 'committed') {
      assistantPhase.value = 'FAILED'
      proposalMessage.value = '修改没有完成，请先刷新当前作品后再试。'
      return
    }
    assistantPhase.value = 'EXECUTED'
    proposalMessage.value = '修改已执行，正在重新核对结果。'
    await new Promise(resolve => setTimeout(resolve, 800))
    assistantPhase.value = 'POST_VERIFYING'
    await store.refresh()
    const afterRevision = store.project?.revision ?? null
    const formalReceiptVerified = receipt.receipt?.status === 'REPAIR_VERIFIED'
    if (afterRevision == null || !formalReceiptVerified) {
      assistantPhase.value = 'FAILED'
      proposalMessage.value = '修改已经执行，但重新检查未完成，请稍后重试。'
      return
    }
    assistantPhase.value = 'VERIFIED'
    proposalMessage.value = '修改已完成，并已重新核对结果。'
  } catch (error) {
    assistantPhase.value = 'FAILED'
    proposalMessage.value = productErrorMessage(error, '修改没有完成，请先刷新当前作品后再试。')
  }
}

const assistantPhaseLabel = computed(() => ({
  PROPOSAL_READY: '修改方案已生成',
  AWAITING_AUTHORIZATION: '等待你的确认',
  EXECUTING: '正在执行修改',
  EXECUTED: '修改已执行',
  POST_VERIFYING: '正在重新核对',
  VERIFIED: '修改已完成并核对通过',
  FAILED: '修改未完成',
}[assistantPhase.value]))

function startConversation() {
  presentation.value = null
  proposal.value = null
  proposalOpen.value = false
  proposalMessage.value = ''
  assistantError.value = ''
  assistantPhase.value = 'PROPOSAL_READY'
  executionBeforeRevision.value = null
  evidenceOpen.value = false
  lastQuestion.value = ''
}
</script>

<template>
  <aside class="prototype-assistant" :class="{ collapsed }" aria-label="项目助手" :data-assistant-state="assistantPhase">
    <div class="prototype-assistanthead">
      <div class="prototype-logo" style="width:30px;height:30px;border-radius:9px">墨</div>
      <div class="prototype-atitle"><strong>项目助手</strong><small>当前作品：{{ projectTitle }} · 正在查看：{{ context }}</small></div>
      <div class="prototype-assistanttools"><button class="prototype-iconbtn" type="button" title="新对话" @click="startConversation">＋</button></div>
      <button class="prototype-iconbtn" type="button" :title="collapsed ? '展开助手' : '折叠助手'" :aria-label="collapsed ? '展开助手' : '折叠助手'" @click="emit('toggle')">{{ collapsed ? '‹' : '›' }}</button>
    </div>

    <div class="prototype-messages">
      <template v-if="presentation">
        <div class="prototype-msg user">{{ lastQuestion }}</div>
        <div class="prototype-msg assistantmsg"><div class="prototype-answercard"><h4>{{ presentation.status === 'supported' ? '已完成当前记录核对' : '还需要继续核对' }}</h4><p>{{ presentation.answer }}</p><div v-if="presentation.evidence.length" class="prototype-evidence"><span>依据来自当前作品记录</span><button class="prototype-link" type="button" @click="evidenceOpen = !evidenceOpen">{{ evidenceOpen ? '收起依据' : '查看依据' }}</button></div><div v-if="evidenceOpen" class="prototype-proposal"><strong>已核对的依据</strong><p v-for="item in presentation.evidence" :key="`${item.label}-${item.source ?? ''}`">{{ item.label }} · {{ item.detail }}</p></div><div class="prototype-proposal"><strong>需要修改时</strong><p>我可以整理一份修改方案，但不会直接改动已经确认的故事事实。</p><button class="prototype-btn" type="button" @click="prepareProposal">查看修改方案</button></div></div></div>
      </template>
      <div v-else class="prototype-msg assistantmsg"><div class="prototype-answercard"><h4>从一件具体的事开始</h4><p>我会先核对当前作品记录，再告诉你能够确认到什么。涉及正式修改时，会先给出方案并等待你的确认。</p><div class="prototype-evidence"><span>当前上下文：{{ context }}</span><span>等待提问</span></div></div></div>
      <div v-if="assistantError" class="prototype-msg assistantmsg" role="alert"><div class="prototype-answercard"><h4>暂时无法完成查询</h4><p>{{ assistantError }}</p><button class="prototype-btn" type="button" @click="assistantError = ''">知道了</button></div></div>
    </div>

    <div v-if="proposalOpen" class="prototype-assistant-plan">
      <div v-if="proposal" class="prototype-answercard">
        <div class="prototype-plan-title"><strong>修改方案</strong><span class="prototype-badge" :class="assistantPhase === 'VERIFIED' ? 'brand' : assistantPhase === 'FAILED' ? 'warn' : 'brand'">{{ assistantPhaseLabel }}</span></div>
        <p>{{ sanitizeProductText(proposal.diagnosis?.claim, '已完成当前作品记录核对。') }}</p>
        <div class="prototype-plan-grid"><span>影响范围</span><strong>{{ displayScopes(proposal.affectedScopes) }}</strong><span>风险</span><strong>{{ sanitizeProductText(proposal.riskLevel, '按当前记录评估') }}</strong><span>执行前备份</span><strong>{{ proposal.requiresBackup ? '需要' : '不需要' }}</strong></div>
        <p v-if="proposalMessage" class="prototype-plan-message">{{ proposalMessage }}</p>
        <div class="prototype-composerbar"><button class="prototype-btn" type="button" @click="proposalOpen = false">暂不处理</button><button v-if="proposal.action === 'EXECUTABLE' && assistantPhase === 'AWAITING_AUTHORIZATION'" class="prototype-btn primary" type="button" :disabled="proposalState.status !== 'committed' || authorizationState.status === 'submitting' || executionState.status === 'submitting'" @click="authorizeProposal">确认执行</button><button v-else-if="proposal.action !== 'EXECUTABLE'" class="prototype-btn" type="button" @click="router.push(`/projects/${store.currentProjectId}/history`)">进入变更记录</button></div>
      </div>
      <p v-else class="prototype-plan-message">{{ proposalMessage }}</p>
    </div>

    <div class="prototype-composer"><div class="prototype-inputbox"><textarea v-model="question" aria-label="向项目助手提问" :placeholder="placeholder" :disabled="sending" @keydown.enter.exact.prevent="send" /><div class="prototype-composerbar"><span class="prototype-hint">Shift + Enter 换行</span><button class="prototype-btn primary" type="button" :disabled="!question.trim() || sending" @click="send">{{ sending ? '核对中…' : '发送' }}</button></div></div></div>
  </aside>
</template>
