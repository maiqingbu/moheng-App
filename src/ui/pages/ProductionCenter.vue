<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import RuntimeBanner from '../components/RuntimeBanner.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const project = computed(() => store.project)
const production = computed(() => store.production)
const lastCommand = ref('none')
const commandState = computed(() => store.commandState(lastCommand.value))
const qualityDimensions = computed(() => Object.entries(production.value?.quality?.dimensions ?? {}))
const qualityFindings = computed(() => production.value?.quality?.findings ?? [])
const stageLabel = (value: unknown) => ({ OPENING: '开篇准备', OPENING_PRODUCTION: '开篇创作', VOLUME_PRODUCTION: '长篇创作', IDLE: '等待开始' } as Record<string, string>)[String(value)] ?? '进行中'
const gateLabel = (value: unknown) => ({ PASS: '已通过', BLOCKED: '暂时阻断', PENDING: '等待检查', FAIL: '未通过' } as Record<string, string>)[String(value)] ?? '等待检查'
const reconciliationLabel = (value: unknown) => ({ NO_ACTION: '无需处理', CONTINUE: '继续', PAUSE: '暂时暂停', ESCALATE: '需要确认' } as Record<string, string>)[String(value)] ?? '等待判断'
const capabilityLabel = (value: unknown) => ({ RENDER_CHAPTER: '撰写章节', AUDIT_EXTRACT: '检查章节', QUALITY_EVALUATION: '评估叙事质量' } as Record<string, string>)[String(value)] ?? '创作步骤'
const statusLabel = (value: unknown) => ({ done: '已完成', active: '进行中', locked: '待解锁', PASS: '已通过', PROPOSED: '待确认', AUTHORIZED: '已授权', REPAIR_VERIFIED: '已核对完成' } as Record<string, string>)[String(value)] ?? '待检查'
const dimensionLabel = (value: unknown) => ({ agency: '主角主动性', desire: '主角欲望', hook: '开篇吸引力', pacing: '节奏', voice: '叙事声音', dialogue: '对话', subtext: '潜台词', repetition: '重复风险', exposition: '信息呈现', emotionalPayoff: '情绪回报' } as Record<string, string>)[String(value)] ?? '叙事维度'
const historicalB10Terms = ['ReaderDesire', 'SOFT 质量评估', 'HARD Gate']

onMounted(() => void store.loadProduction())

async function startProduction() {
  const chapter = production.value?.nextChapter
  if (!chapter) throw new Error('B08_CHAPTER_REQUIRED')
  lastCommand.value = 'production-start'
  await store.runCommand(lastCommand.value, {
    type: 'production_start',
    payload: {
      chapterId: chapter.id,
      volumeId: chapter.volumeId,
      executionMode: chapter.executionMode,
      entityIds: [],
      references: [],
      skillsEnabled: true,
    },
  })
}

async function controlProduction(type: 'production_pause' | 'production_resume' | 'production_cancel') {
  lastCommand.value = type
  await store.runCommand(lastCommand.value, {
    type,
    payload: production.value?.tasks[0]?.id ? { jobId: production.value.tasks[0].id } : {},
  })
}

async function authorizeRepair() {
  const repair = production.value?.repair
  if (!repair || repair.status !== 'PROPOSED') return
  const authorizationId = `repair-auth:${repair.repairPlanId}:${Date.now()}`
  lastCommand.value = 'repair-authorize'
  await store.runCommand(lastCommand.value, {
    type: 'repair_authorize',
    payload: {
      authorization: {
        authorizationId,
        projectId: project.value?.id,
        repairPlanId: repair.repairPlanId,
        basedOnProjectRevision: repair.basedOnProjectRevision,
        operationDigest: repair.operationDigest,
        approvedByUser: true,
        issuedAt: new Date().toISOString(),
      },
    },
  })
}

async function executeRepair() {
  const repair = production.value?.repair
  if (!repair?.authorizationId || repair.status !== 'AUTHORIZED') return
  lastCommand.value = 'repair-execute'
  await store.runCommand(lastCommand.value, {
    type: 'repair_execute',
    payload: { repairPlanId: repair.repairPlanId, authorizationId: repair.authorizationId },
  })
}
</script>

<template>
  <div v-if="project && production" class="page">
    <PageHeader title="创作中心" subtitle="章节生产与授权修复均从当前作品记录重新查询。">
      <button v-if="project.runState !== 'ACTIVE' && production.nextChapter" class="btn primary" @click="startProduction">开始第 {{ production.nextChapter.number }} 章</button>
      <button v-if="project.runState === 'ACTIVE'" class="btn" @click="controlProduction('production_pause')">暂停</button>
      <button v-if="project.runState === 'PAUSED'" class="btn primary" @click="controlProduction('production_resume')">恢复</button>
    </PageHeader>
    <RuntimeBanner :state="project.runState" :reason="project.blockReason" />
    <CommandFeedback :state="commandState" />

    <div class="metric-grid section">
      <MetricCard label="当前阶段" :value="stageLabel(production.productionStage)" :note="production.activeWork.detail" />
      <MetricCard label="正式章节" :value="project.committedChapters" note="只有正式确认的正文会计入这里" />
      <MetricCard label="准入检查" :value="gateLabel(production.gate?.status)" :note="production.gate?.reason || '等待结构化校验'" />
      <MetricCard label="后续安排" :value="reconciliationLabel(production.reconciliation)" note="系统会根据当前记录给出下一步" />
    </div>

    <div class="production-layout section">
      <section class="card pad">
        <div class="card-head"><div><h2 class="card-title">生产时间线</h2><p class="card-subtitle">上下文 → 规划 → 撰写 → 检查 → 准入 → 提交 → 后续安排</p></div></div>
        <div class="timeline">
          <div v-for="item in production.timeline" :key="item.id" class="timeline-item" :class="item.status">
            <div class="timeline-marker">{{ item.status === 'done' ? '✓' : item.order }}</div>
            <div><strong>{{ item.title }}</strong><div class="muted" style="font-size:12px">{{ item.description }}</div></div>
          </div>
        </div>
      </section>
      <aside style="display:grid;gap:16px">
        <section class="card pad">
          <h2 class="card-title">当前章节规划</h2>
          <template v-if="production.chapterPlan">
            <p><strong>{{ production.chapterPlan.chapterGoal }}</strong></p>
            <small class="muted">当前规划已与作品版本绑定</small>
            <p class="muted">依据范围：当前任务所需的作品记录</p>
            <p class="muted">规划状态：{{ production.stale?.status === 'PLAN_STALE' ? '需要重新检查' : '可以使用' }}<span v-if="production.stale?.reason"> · {{ production.stale.reason }}</span></p>
            <div v-for="item in production.triggeredContinuity" :key="item.threadRef" class="task-row"><strong>相关剧情线</strong><small>{{ item.bridge }}</small></div>
          </template>
          <p v-else class="muted">开篇阶段会先整理当前所需信息，再建立第一章规划。</p>
        </section>
        <section class="card pad">
          <h2 class="card-title">创作步骤与结果</h2>
          <div v-for="run in production.modelRuns" :key="run.modelRunId" class="task-row"><strong>{{ capabilityLabel(run.capability) }}</strong><small class="muted">已使用当前配置 · {{ statusLabel(run.status) }}</small></div>
          <p class="muted">制作稿：{{ production.candidateRef ? '已生成，等待检查' : '无' }}</p>
          <p class="muted">正式正文：{{ production.finalChapterRef ? '已提交' : '无' }}</p>
        </section>
      </aside>
    </div>

    <section v-if="production.creativeSkills || production.quality" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">创作建议与叙事质量</h2><p class="card-subtitle">建议用于改善正文表现；正式准入仍以作品记录和结构化检查为准。</p></div>
        <span class="muted">{{ production.quality?.softOnly ? '辅助质量评估' : '等待质量评估' }}</span>
      </div>
      <div v-if="production.creativeSkills" class="metric-grid">
        <MetricCard label="创作建议状态" value="已加载" note="使用当前作品的受限建议" />
        <MetricCard label="主要建议" :value="production.creativeSkills.selection?.primary?.nameZh || '无'" note="辅助参考" />
        <MetricCard label="补充建议数" :value="String(production.creativeSkills.selection?.support?.length || 0)" note="只展示当前任务需要的内容" />
        <MetricCard label="本次使用" :value="String(production.creativeSkills.invocations?.length || 0)" note="已记录到当前作品" />
      </div>
      <div v-if="production.creativeSkills" class="task-row">
        <strong>当前作品版本 {{ production.creativeSkills.context?.projectRevision ?? '-' }}</strong>
        <small class="muted">已选择 {{ production.creativeSkills.context?.boundedRefs?.length || 0 }} 项必要记录 · 本次使用 {{ production.creativeSkills.context?.budget?.modelCalls || 0 }} 次创作调用</small>
      </div>
      <div v-if="production.quality" class="quality-grid">
        <div v-for="([dimension, item]) in qualityDimensions" :key="dimension" class="task-row">
          <strong>{{ dimensionLabel(dimension) }}</strong><span>{{ item.score }} 分 · {{ item.severity === 'risk' ? '需重点关注' : item.severity === 'attention' ? '建议关注' : '稳定' }}</span>
        </div>
      </div>
      <div v-if="production.quality" class="task-row">
        <strong>读者期待：{{ production.quality.softAssessment?.readerDesire || '等待评估' }}</strong>
        <small class="muted">当前偏好版本已记录 · 本次评估 {{ production.quality.modelProvenance?.evaluatorCalls || 0 }} 次</small>
      </div>
      <div v-if="production.quality?.repair" class="task-row">
        <strong>局部修订 {{ production.quality.repair.run }}/{{ production.quality.repair.limit }}</strong>
        <small class="muted">影响范围：{{ production.quality.repair.scope.join('、') || '无' }} · {{ production.quality.repair.escalation || '未升级' }}</small>
      </div>
      <div v-for="finding in qualityFindings" :key="finding.findingId" class="task-row">
        <strong>{{ dimensionLabel(finding.dimension) }}</strong><small class="muted">{{ finding.repairable ? '可以做局部修订' : '仅供查看' }}</small>
      </div>
    </section>

    <section v-if="production.repair" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">需要确认的修复</h2><p class="card-subtitle">系统会先说明影响范围；只有你明确确认后才会执行，并在完成后重新核对。</p></div>
        <button v-if="production.repair.status === 'PROPOSED'" class="btn primary" @click="authorizeRepair">明确授权</button>
        <button v-if="production.repair.status === 'AUTHORIZED' && !production.repair.consumedAt" class="btn primary" @click="executeRepair">执行一次性修复</button>
      </div>
      <div class="metric-grid">
        <MetricCard label="状态" :value="statusLabel(production.repair.status)" :note="production.repair.authorizationId ? '已获得本次确认' : '尚未确认'" />
        <MetricCard label="影响等级" :value="production.repair.riskLevel === 'LOW' ? '较低' : '需要关注'" :note="production.repair.requiresBackup ? '执行前需要备份' : '无需备份'" />
        <MetricCard label="核对范围" :value="production.repair.affectedScopes.join('、') || '当前作品'" note="执行后会再次核对" />
      </div>
      <p><strong>影响范围：</strong>{{ production.repair.affectedScopes.join('、') || '无' }}</p>
      <p><strong>处理方式：</strong>重新整理并核对当前作品展示。</p>
      <div v-if="production.repair.executionReceipt" class="task-row">
        <strong>{{ statusLabel(production.repair.executionReceipt.status) }}</strong><small class="muted">修复结果已经重新核对</small>
      </div>
    </section>
  </div>
</template>
