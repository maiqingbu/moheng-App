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
const repairDiagnosis = computed(() => JSON.stringify(production.value?.repair?.diagnosis ?? {}, null, 2))
const qualityDimensions = computed(() => Object.entries(production.value?.quality?.dimensions ?? {}))
const qualityFindings = computed(() => production.value?.quality?.findings ?? [])

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
    <PageHeader title="AI 制片中心" subtitle="B08 章节生产与 H03 授权修复均从 SQLite 权威投影重新查询。">
      <button v-if="project.runState !== 'ACTIVE' && production.nextChapter" class="btn primary" @click="startProduction">开始第 {{ production.nextChapter.number }} 章</button>
      <button v-if="project.runState === 'ACTIVE'" class="btn" @click="controlProduction('production_pause')">暂停</button>
      <button v-if="project.runState === 'PAUSED'" class="btn primary" @click="controlProduction('production_resume')">恢复</button>
    </PageHeader>
    <RuntimeBanner :state="project.runState" :reason="project.blockReason" />
    <CommandFeedback :state="commandState" />

    <div class="metric-grid section">
      <MetricCard label="当前阶段" :value="production.productionStage || 'IDLE'" :note="production.activeWork.detail" />
      <MetricCard label="正式章节" :value="project.committedChapters" note="ChapterCandidate 不进入正式计数" />
      <MetricCard label="Final Gate" :value="production.gate?.status || 'PENDING'" :note="production.gate?.reason || '等待结构化校验'" />
      <MetricCard label="Reconciliation" :value="production.reconciliation || 'NO_ACTION'" note="B08 只分类，不执行 B09/B11" />
    </div>

    <div class="production-layout section">
      <section class="card pad">
        <div class="card-head"><div><h2 class="card-title">生产时间线</h2><p class="card-subtitle">Context → Plan → Render → Audit → Gate → Commit → Reconcile</p></div></div>
        <div class="timeline">
          <div v-for="item in production.timeline" :key="item.id" class="timeline-item" :class="item.status">
            <div class="timeline-marker">{{ item.status === 'done' ? '✓' : item.order }}</div>
            <div><strong>{{ item.title }}</strong><div class="muted" style="font-size:12px">{{ item.description }}</div></div>
          </div>
        </div>
      </section>
      <aside style="display:grid;gap:16px">
        <section class="card pad">
          <h2 class="card-title">Current ChapterPlan</h2>
          <template v-if="production.chapterPlan">
            <p><strong>{{ production.chapterPlan.chapterGoal }}</strong></p>
            <small class="muted">{{ production.chapterPlan.ref.artifactId }} · v{{ production.chapterPlan.ref.version }}</small>
            <p class="muted">Context: {{ production.chapterPlan.contextManifestRef?.artifactId }}</p>
            <p class="muted">Stale: {{ production.stale?.status }} {{ production.stale?.reason }}</p>
            <div v-for="item in production.triggeredContinuity" :key="item.threadRef" class="task-row"><strong>{{ item.threadRef }}</strong><small>{{ item.disposition }} {{ item.bridge }}</small></div>
          </template>
          <p v-else class="muted">开篇模式将先由 B06 编译 Context，再由 B07 owner 创建 ChapterPlan。</p>
        </section>
        <section class="card pad">
          <h2 class="card-title">ModelRun 与产物</h2>
          <div v-for="run in production.modelRuns" :key="run.modelRunId" class="task-row"><strong>{{ run.capability }}</strong><small class="muted">{{ run.provider }} / {{ run.model }} · {{ run.status }}</small></div>
          <p class="muted">Candidate: {{ production.candidateRef?.artifactId || '无' }}</p>
          <p class="muted">Final: {{ production.finalChapterRef?.artifactId || '无' }}</p>
        </section>
      </aside>
    </div>

    <section v-if="production.creativeSkills || production.quality" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">Creative Skills 与叙事质量</h2><p class="card-subtitle">Skill 只提供有界创作建议；Quality 与 ReaderDesire 是 SOFT 信号，HARD Gate 仍由 B08/B06–B09 权威执行。</p></div>
        <span class="muted">{{ production.quality?.softOnly ? 'SOFT 质量评估' : '等待质量评估' }}</span>
      </div>
      <div v-if="production.creativeSkills" class="metric-grid">
        <MetricCard label="Skill 包" :value="production.creativeSkills.packageVersion" :note="production.creativeSkills.packageDigest" />
        <MetricCard label="Primary" :value="production.creativeSkills.selection?.primary?.id || '无'" :note="production.creativeSkills.selection?.primary?.nameZh || ''" />
        <MetricCard label="Support" :value="String(production.creativeSkills.selection?.support?.length || 0)" note="默认最多 3 个" />
        <MetricCard label="Invocation" :value="String(production.creativeSkills.invocations?.length || 0)" note="来自权威投影的 provenance" />
      </div>
      <div v-if="production.creativeSkills" class="task-row">
        <strong>Context {{ production.creativeSkills.context?.projectRevision ?? '-' }} · {{ production.creativeSkills.context?.contextDigest || '无 digest' }}</strong>
        <small class="muted">{{ production.creativeSkills.context?.boundedRefs?.length || 0 }} 个 bounded refs · {{ production.creativeSkills.context?.budget?.contextTokens || 0 }} tokens · {{ production.creativeSkills.context?.budget?.modelCalls || 0 }} model calls</small>
      </div>
      <div v-if="production.quality" class="quality-grid">
        <div v-for="([dimension, item]) in qualityDimensions" :key="dimension" class="task-row">
          <strong>{{ dimension }}</strong><span>{{ item.score }}/100 · {{ item.severity }}</span>
        </div>
      </div>
      <div v-if="production.quality" class="task-row">
        <strong>ReaderDesire：{{ production.quality.softAssessment?.readerDesire || 'SOFT' }}</strong>
        <small class="muted">Taste {{ production.quality.tasteProfileVersion || production.tasteProfile?.version || '-' }} · merged evaluator calls {{ production.quality.modelProvenance?.evaluatorCalls || 0 }}</small>
      </div>
      <div v-if="production.quality?.repair" class="task-row">
        <strong>Candidate 修复 {{ production.quality.repair.run }}/{{ production.quality.repair.limit }}</strong>
        <small class="muted">scope: {{ production.quality.repair.scope.join('、') || '无' }} · {{ production.quality.repair.escalation || '未升级' }}</small>
      </div>
      <div v-for="finding in qualityFindings" :key="finding.findingId" class="task-row">
        <strong>{{ finding.dimension }} · {{ finding.severity }}</strong><small class="muted">{{ finding.repairable ? '可做 Candidate 局部修复' : '仅供审阅' }}</small>
      </div>
    </section>

    <section v-if="production.repair" class="card pad section">
      <div class="card-head">
        <div><h2 class="card-title">H03 显式授权修复</h2><p class="card-subtitle">RepairPlan 不是权限；只有用户点击才签发一次性 P3 Authorization。</p></div>
        <button v-if="production.repair.status === 'PROPOSED'" class="btn primary" @click="authorizeRepair">明确授权</button>
        <button v-if="production.repair.status === 'AUTHORIZED' && !production.repair.consumedAt" class="btn primary" @click="executeRepair">执行一次性修复</button>
      </div>
      <div class="metric-grid">
        <MetricCard label="状态" :value="production.repair.status" :note="production.repair.authorizationId || '尚未授权'" />
        <MetricCard label="风险" :value="production.repair.riskLevel" :note="production.repair.requiresBackup ? '执行前需要备份' : '无需备份'" />
        <MetricCard label="Owner" :value="production.repair.targetAuthorityOwner" :note="production.repair.operationDigest" />
      </div>
      <pre class="muted" style="white-space:pre-wrap">{{ repairDiagnosis }}</pre>
      <p><strong>影响范围：</strong>{{ production.repair.affectedScopes.join('、') || '无' }}</p>
      <p><strong>操作：</strong>{{ production.repair.proposedOperations.map(item => item.operation).join('、') }}</p>
      <p><strong>Postconditions：</strong>{{ JSON.stringify(production.repair.expectedPostconditions) }}</p>
      <div v-if="production.repair.executionReceipt" class="task-row">
        <strong>{{ production.repair.executionReceipt.status }}</strong><small class="muted">{{ production.repair.executionReceipt.receiptRef }}</small>
        <pre class="muted" style="white-space:pre-wrap">{{ JSON.stringify(production.repair.executionReceipt.postconditions, null, 2) }}</pre>
      </div>
    </section>
  </div>
</template>
