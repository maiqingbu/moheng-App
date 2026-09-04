<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import BaseModal from '../components/BaseModal.vue'
import { useProjectStore } from '../stores/projectStore'
import { presentAssistantSession } from '../presentation/assistantPresentation'
import type { AssistantPresentation } from '../presentation/assistantPresentation'
import {
  createGroundedAssistantRetcon,
  RetconContextSelectionError,
  startDependencySummaryBootstrap,
} from '../flows/b12RetconFlow'
import type { RetconFactOption } from '../flows/b12RetconFlow'
import { productErrorMessage, sanitizeProductText } from '../presentation/productLanguage'

const store = useProjectStore()
const route = useRoute()
const router = useRouter()
const question = ref('当前作品的进度和故事核心是否一致？')
const presentation = ref<AssistantPresentation | null>(null)
type AssistantPlan = Awaited<ReturnType<typeof store.prepareAssistantRepair>>
const plan = ref<AssistantPlan | null>(null)
const evidenceOpen = ref(false)
const repairOpen = ref(false)
const repairMessage = ref('')
const changeMessage = ref('')
const queryState = computed(() => store.commandState('assistant-query'))
const planState = computed(() => store.commandState('assistant-plan'))
const authState = computed(() => store.commandState('assistant-authorize'))
const executeState = computed(() => store.commandState('assistant-execute'))
const groundingOptions = ref<RetconFactOption[]>([])
const selectedGroundingFactId = ref('')
const productUxEvidenceMode = computed(() => import.meta.env.VITE_BROWSER_DEMO === 'true' ? String(route.query.productUxEvidence ?? '') : '')
const displayScopes = (values: unknown) => Array.isArray(values)
  ? values.map(value => sanitizeProductText(value, '当前作品')).filter(Boolean).join('、') || '无'
  : '无'

async function ask() {
  const session = await store.supportSession(question.value, 'DIAGNOSE')
  if (session) presentation.value = presentAssistantSession(session)
}

async function prepareRepair() {
  try {
    plan.value = await store.prepareAssistantRepair(question.value)
  } catch (error) {
    repairMessage.value = productErrorMessage(error, '暂时无法核对修复范围，请刷新当前作品后重试。')
    repairOpen.value = true
    return
  }
  repairMessage.value = plan.value.action === 'PROPOSAL_ONLY'
    ? '这项请求会改变正式事实，请先走修改提案。'
    : plan.value.action === 'NO_ACTION' ? '当前记录不支持执行修复。' : ''
  repairOpen.value = true
}

async function authorizeRepair() {
  if (!plan.value || plan.value.action !== 'EXECUTABLE') return
  try {
    const execute = await store.authorizeAssistantRepair(plan.value)
    repairMessage.value = execute.finalStatus === 'committed' ? '修复已完成，并已重新核对结果。' : '修复没有完成，请先刷新当前作品后再试。'
  } catch (error) {
    repairMessage.value = productErrorMessage(error, '修复没有完成，请先刷新当前作品后再试。')
  }
}

async function proposeChange() {
  const projectId = store.currentProjectId
  if (!projectId || !question.value.trim()) return
  const changeId = `change:${projectId}:${Date.now()}`
  const changeType = /事实|改成|原来|没有死|其实/.test(question.value) ? 'RETCON' : 'CHANGE'
  try {
    if (changeType === 'RETCON') {
      const grounded = await createGroundedAssistantRetcon({
        execute: (key, request) => store.runCommand(key, request),
        queryRetconContext: request => store.queryRetconContext(String(request.query ?? '')),
      }, projectId, question.value, selectedGroundingFactId.value || undefined)
      const preflight = grounded.preflight
      if (preflight.status === 'SUMMARY_BOOTSTRAP_REQUIRED') {
        changeMessage.value = '正在准备全局影响索引，请稍候。'
        const bootstrap = await startDependencySummaryBootstrap(
          (key, request) => store.runCommand(key, request),
          projectId,
        )
        if (bootstrap.status !== 'READY') {
          router.push({ path: `/projects/${projectId}/history`, query: { changeRequestId: grounded.changeRequestId, bootstrap: '1' } })
          return
        }
        await store.runCommand('assistant-change-impact', { type: 'impact_preflight', projectId, payload: { changeRequestId: grounded.changeRequestId } })
      }
      changeMessage.value = `已找到并核对“${grounded.fact.label}”，依据为“${grounded.evidence.label}”；事实修订提案已建立，请到版本与变更记录查看方案并完成二次确认。`
      groundingOptions.value = []
      selectedGroundingFactId.value = ''
      router.push({ path: `/projects/${projectId}/history`, query: { changeRequestId: grounded.changeRequestId } })
      return
    }
    await store.runCommand('assistant-change-proposal', {
      type: 'change_request_create',
      projectId,
      payload: {
        changeRequestId: changeId,
        changeType,
        scope: 'PROJECT',
        summary: question.value.trim(),
        reason: '用户从项目助手明确发起变更提案',
        requestedChange: { description: question.value.trim(), source: 'PROJECT_ASSISTANT' },
      },
    })
    const preflight = await store.runCommand('assistant-change-impact', {
      type: 'impact_preflight',
      projectId,
      payload: { changeRequestId: changeId },
    })
    const result = preflight.result ?? {}
    changeMessage.value = `变更提案已建立，预计影响范围：${result.scopeClass === 'GLOBAL' ? '全局' : result.scopeClass === 'WIDE' ? '较广' : '局部'}；请到版本与变更记录完成最终核对。`
    router.push({ path: `/projects/${projectId}/history`, query: { changeRequestId: changeId } })
  } catch (error) {
    if (error instanceof RetconContextSelectionError) {
      groundingOptions.value = error.candidates
      selectedGroundingFactId.value = ''
      changeMessage.value = error.message
      return
    }
    changeMessage.value = productErrorMessage(error, '变更提案未建立，请刷新当前作品后重试。')
  }
}

function chooseGrounding(factId: string) {
  selectedGroundingFactId.value = factId
  void proposeChange()
}

onMounted(() => {
  if (productUxEvidenceMode.value !== 'm11') return
  presentation.value = presentAssistantSession({
    plan: {},
    finding: { claim: '当前查询需要继续核对。', supportStatus: 'INSUFFICIENT', sourceRefs: ['diagnosis:demo'] },
    calls: [{ capabilityId: 'query_continuity', callId: 'r1r2-diagnosis', result: [{ chapterId: 'chapter:18', status: 'OPEN' }] }],
    evidenceLedger: [{ callId: 'r1r2-diagnosis', sourceRefs: ['diagnosis:demo'], startedAt: '2026-09-03T02:00:00Z', finishedAt: '2026-09-03T02:00:01Z' }],
    receipt: { projectRevision: 18 },
  })
})
</script>

<template>
  <div class="page">
    <PageHeader title="项目助手" subtitle="只回答当前作品记录中能够核对的内容；涉及正式修改时，必须先查看方案并由你明确授权。">
      <button class="btn" @click="router.push(`/projects/${store.currentProjectId ?? ''}`)">返回作品总览</button>
    </PageHeader>
    <section class="card pad">
      <label class="field"><span class="field-label">你想确认什么</span><textarea v-model="question" class="textarea" /></label>
      <div class="actions" style="justify-content:flex-end;margin-top:14px"><button class="btn primary" @click="ask">查询当前作品记录</button><button class="btn" @click="prepareRepair">准备一次可核对的修复</button><button class="btn warning" @click="proposeChange">建立变更提案</button></div>
      <CommandFeedback :state="queryState" />
      <p v-if="changeMessage" class="banner info" style="margin-top:12px">{{ changeMessage }}</p>
      <div v-if="groundingOptions.length" class="section" style="display:grid;gap:8px;margin-top:12px"><span class="muted">请选择要修订的当前事实：</span><button v-for="fact in groundingOptions" :key="fact.factId" class="btn" @click="chooseGrounding(fact.factId)">{{ fact.label }}</button></div>
    </section>

    <section v-if="presentation" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">查询结果</h2><p class="card-subtitle">回答来自当前作品的已核对记录。</p></div><span class="badge" :class="presentation.status === 'supported' ? 'pine' : 'warning'">{{ presentation.status === 'supported' ? '有记录支持' : '需要继续核对' }}</span></div>
      <p style="font-size:16px;line-height:1.8">{{ presentation.answer }}</p>
      <div class="section" style="display:grid;gap:10px"><div v-for="section in presentation.sections" :key="section.title" class="task-row"><strong>{{ section.title }}</strong><span>{{ section.body }}</span></div></div>
      <button class="btn small" @click="evidenceOpen=true">查看依据</button>
    </section>

    <section v-if="productUxEvidenceMode === 'm11'" class="card pad section" data-product-ux-state="diagnosis">
      <div class="card-head"><div><h2 class="card-title">诊断结果</h2><p class="card-subtitle">当前查询需要继续核对。</p></div><span class="badge warning">需要关注</span></div>
      <p>系统已完成当前作品记录检查；暂时不能确认这项内容。</p>
      <div class="task-row"><strong>下一步</strong><span>可以补充相关章节或设定依据，再重新确认。</span></div>
    </section>

    <BaseModal :open="evidenceOpen" title="已核验的项目记录" subtitle="这里只展示依据的中文摘要，不展示内部编号或执行细节。" @close="evidenceOpen=false">
      <div v-if="presentation?.evidence.length" class="section" style="display:grid;gap:10px"><div v-for="item in presentation.evidence" :key="`${item.label}-${item.source ?? ''}`" class="task-row"><strong>{{ item.label }}</strong><span>{{ item.detail }}<small v-if="item.source || item.record || item.currentStatus || item.related || item.recordedAt" class="muted" style="display:block;margin-top:4px">{{ [item.source, item.record, item.currentStatus ? `状态：${item.currentStatus}` : '', item.related, item.recordedAt ? `记录时间：${item.recordedAt}` : ''].filter(Boolean).join(' · ') }}</small></span></div></div>
      <div v-else class="empty">当前没有足够的依据可以展开。</div>
    </BaseModal>

    <BaseModal :open="repairOpen" title="确认修复范围" subtitle="方案只会影响已说明的页面信息；正式执行前仍需你的明确授权。" @close="repairOpen=false">
      <div v-if="plan" style="display:grid;gap:12px"><div class="task-row"><strong>核对结果</strong><span>{{ plan.action === 'EXECUTABLE' ? '可以执行这项修复' : plan.action === 'PROPOSAL_ONLY' ? '需要先建立正式修改提案' : '当前记录不需要执行修复' }}</span></div><div class="task-row"><strong>诊断</strong><span>{{ sanitizeProductText(plan.diagnosis.claim, '已完成当前作品记录核对。') }}</span></div><div class="task-row"><strong>影响范围</strong><span>{{ displayScopes(plan.affectedScopes) }}</span></div><div class="task-row"><strong>执行前备份</strong><span>{{ plan.requiresBackup ? '需要' : '不需要' }}</span></div></div>
      <CommandFeedback :state="planState" />
      <CommandFeedback :state="authState" />
      <CommandFeedback :state="executeState" />
      <p v-if="repairMessage" class="banner" :class="repairMessage.includes('完成') ? 'success' : 'warning'">{{ repairMessage }}</p>
      <template #footer><button class="btn" @click="repairOpen=false">暂不执行</button><button class="btn primary" :disabled="!plan || plan.action !== 'EXECUTABLE' || planState.status !== 'committed' || ['submitting','accepted','processing'].includes(authState.status)" @click="authorizeRepair">我确认并执行修复</button></template>
    </BaseModal>
  </div>
</template>
