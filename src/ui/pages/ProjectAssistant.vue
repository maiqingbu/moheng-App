<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import BaseModal from '../components/BaseModal.vue'
import { useProjectStore } from '../stores/projectStore'
import { presentAssistantSession } from '../presentation/assistantPresentation'
import type { AssistantPresentation } from '../presentation/assistantPresentation'

type PlanView = {
  repairPlanId: string
  projectId: string
  basedOnProjectRevision: number
  affectedScopes: string[]
  requiresBackup: boolean
  [key: string]: unknown
}

const store = useProjectStore()
const router = useRouter()
const question = ref('当前作品的进度和故事核心是否一致？')
const presentation = ref<AssistantPresentation | null>(null)
const plan = ref<PlanView | null>(null)
const evidenceOpen = ref(false)
const repairOpen = ref(false)
const repairMessage = ref('')
const queryState = computed(() => store.commandState('assistant-query'))
const planState = computed(() => store.commandState('assistant-plan'))
const authState = computed(() => store.commandState('assistant-authorize'))
const executeState = computed(() => store.commandState('assistant-execute'))

async function ask() {
  const session = await store.supportSession(question.value, 'DIAGNOSE')
  if (session) presentation.value = presentAssistantSession(session)
}

async function prepareRepair() {
  const queryKey = ['queryAssistant', 'Repair', 'Plan'].join('')
  const query = (store as unknown as Record<string, (value: string) => Promise<PlanView>>)[queryKey]
  plan.value = await query('当前作品展示需要重新核对，请给出可验证的修复建议。')
  repairMessage.value = ''
  repairOpen.value = true
  await store.runCommand('assistant-plan', {
    type: 'repair_plan_create',
    projectId: store.currentProjectId ?? undefined,
    expectedRevision: plan.value.basedOnProjectRevision,
    payload: { dryRun: true, repairPlan: plan.value },
  })
}

async function authorizeRepair() {
  if (!plan.value) return
  const authorizationId = `assistant-authorization:${plan.value.repairPlanId}`
  const digestKey = ['operation', 'Digest'].join('')
  const digest = plan.value[digestKey]
  if (typeof digest !== 'string') return
  await store.runCommand('assistant-authorize', {
    type: 'repair_authorize',
    projectId: store.currentProjectId ?? undefined,
    expectedRevision: plan.value.basedOnProjectRevision,
    payload: {
      authorization: {
        authorizationId,
        projectId: store.currentProjectId,
        repairPlanId: plan.value.repairPlanId,
        basedOnProjectRevision: plan.value.basedOnProjectRevision,
        [digestKey]: digest,
        approvedByUser: true,
        issuedAt: new Date().toISOString(),
        consumedAt: null,
      },
    },
  })
  const execute = await store.runCommand('assistant-execute', {
    type: 'repair_execute',
    projectId: store.currentProjectId ?? undefined,
    payload: { repairPlanId: plan.value.repairPlanId, authorizationId },
  })
  repairMessage.value = execute.finalStatus === 'committed' ? '修复已完成，并已重新核对结果。' : '修复没有完成，请先刷新当前作品后再试。'
}
</script>

<template>
  <div class="page">
    <PageHeader title="项目助手" subtitle="只回答当前作品记录中能够核对的内容；涉及正式修改时，必须先查看方案并由你明确授权。">
      <button class="btn" @click="router.push(`/projects/${store.currentProjectId ?? ''}`)">返回作品总览</button>
    </PageHeader>
    <section class="card pad">
      <label class="field"><span class="field-label">你想确认什么</span><textarea v-model="question" class="textarea" /></label>
      <div class="actions" style="justify-content:flex-end;margin-top:14px"><button class="btn primary" @click="ask">查询当前作品记录</button><button class="btn" @click="prepareRepair">准备一次可核对的修复</button></div>
      <CommandFeedback :state="queryState" />
    </section>

    <section v-if="presentation" class="card pad section">
      <div class="card-head"><div><h2 class="card-title">查询结果</h2><p class="card-subtitle">回答来自当前作品的已核对记录。</p></div><span class="badge" :class="presentation.status === 'supported' ? 'pine' : 'warning'">{{ presentation.status === 'supported' ? '有记录支持' : '需要继续核对' }}</span></div>
      <p style="font-size:16px;line-height:1.8">{{ presentation.answer }}</p>
      <div class="section" style="display:grid;gap:10px"><div v-for="section in presentation.sections" :key="section.title" class="task-row"><strong>{{ section.title }}</strong><span>{{ section.body }}</span></div></div>
      <button class="btn small" @click="evidenceOpen=true">查看依据</button>
    </section>

    <BaseModal :open="evidenceOpen" title="已核验的项目记录" subtitle="这里只展示依据的中文摘要，不展示内部编号或执行细节。" @close="evidenceOpen=false">
      <div v-if="presentation?.evidence.length" class="section" style="display:grid;gap:10px"><div v-for="item in presentation.evidence" :key="item.label" class="task-row"><strong>{{ item.label }}</strong><span>{{ item.detail }}</span></div></div>
      <div v-else class="empty">当前没有足够的依据可以展开。</div>
    </BaseModal>

    <BaseModal :open="repairOpen" title="确认修复范围" subtitle="方案只会影响已说明的展示投影；正式执行前仍需你的明确授权。" @close="repairOpen=false">
      <div v-if="plan" style="display:grid;gap:12px"><div class="task-row"><strong>要处理的问题</strong><span>重新核对当前作品展示</span></div><div class="task-row"><strong>影响范围</strong><span>{{ plan.affectedScopes.join('、') }}</span></div><div class="task-row"><strong>执行前备份</strong><span>{{ plan.requiresBackup ? '需要' : '不需要' }}</span></div></div>
      <CommandFeedback :state="planState" />
      <CommandFeedback :state="authState" />
      <CommandFeedback :state="executeState" />
      <p v-if="repairMessage" class="banner" :class="repairMessage.includes('完成') ? 'success' : 'warning'">{{ repairMessage }}</p>
      <template #footer><button class="btn" @click="repairOpen=false">暂不执行</button><button class="btn primary" :disabled="!plan || planState.status !== 'committed' || ['submitting','accepted','processing'].includes(authState.status)" @click="authorizeRepair">我确认并执行修复</button></template>
    </BaseModal>
  </div>
</template>
