<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import MetricCard from '../components/MetricCard.vue'
import { useProjectStore } from '../stores/projectStore'
import { destinationLabel, factorLabel, healthEvidenceProjection, healthIssueLocation, healthLevelLabel, issueKey, severityLabel, statusLabel, truthClassLabel } from '../presentation/healthLanguage'
import { isProductUxEvidenceDemoEnabled } from '../presentation/productLanguage'

const store = useProjectStore()
const route = useRoute()
const router = useRouter()
const factorFilter = ref('全部因素')
const severityFilter = ref('全部状态')
const pageHistory = ref<string[]>([])
const factorSourceCursorHistory = ref<Record<string, string[]>>({})
const factorSourceErrors = ref<Record<string, string>>({})
const factorSourcePages = computed(() => store.healthFactorSourcePages)
const evidenceMode = computed(() => isProductUxEvidenceDemoEnabled() ? String(route.query.healthEvidence ?? '') : '')
const health = computed(() => {
  const base = store.health
  return base ? healthEvidenceProjection(base, evidenceMode.value, pageHistory.value.at(-1) ?? null) : null
})
const loading = computed(() => store.isWorkspaceLoading('health'))
const error = computed(() => store.healthError)
const filteredIssues = computed(() => (health.value?.issues.items ?? []).filter(issue =>
  (factorFilter.value === '全部因素' || factorLabel(issue.factorCode) === factorFilter.value)
  && (severityFilter.value === '全部状态' || severityLabel(issue.severity) === severityFilter.value)
))
const factorOptions = computed(() => ['全部因素', ...(health.value?.factors ?? []).map(factor => factorLabel(factor.factorCode))])
const scoreText = computed(() => health.value?.score == null
  ? health.value?.level === 'source_unavailable' ? '暂不可用' : '数据积累中'
  : `${health.value.score} 分`)

function factorSourceCount(factor: NonNullable<typeof health.value>['factors'][number]) {
  return factor.sourceRefsTotal ?? factor.sourceRefs.length
}

function openFactorSources(factorCode: string) {
  factorSourceCursorHistory.value = { ...factorSourceCursorHistory.value, [factorCode]: [] }
  factorSourceErrors.value = { ...factorSourceErrors.value, [factorCode]: '' }
  void store.loadHealthFactorSources(factorCode).catch(() => {
    factorSourceErrors.value = { ...factorSourceErrors.value, [factorCode]: '来源暂时无法读取，请重试。' }
  })
}

function nextFactorSources(factorCode: string) {
  const cursor = factorSourcePages.value[factorCode]?.nextCursor
  if (!cursor) return
  factorSourceCursorHistory.value = {
    ...factorSourceCursorHistory.value,
    [factorCode]: [...(factorSourceCursorHistory.value[factorCode] ?? []), cursor],
  }
  factorSourceErrors.value = { ...factorSourceErrors.value, [factorCode]: '' }
  void store.loadHealthFactorSources(factorCode, cursor).catch(() => {
    factorSourceErrors.value = { ...factorSourceErrors.value, [factorCode]: '来源暂时无法读取，请重试。' }
  })
}

function previousFactorSources(factorCode: string) {
  const history = [...(factorSourceCursorHistory.value[factorCode] ?? [])]
  history.pop()
  factorSourceCursorHistory.value = { ...factorSourceCursorHistory.value, [factorCode]: history }
  factorSourceErrors.value = { ...factorSourceErrors.value, [factorCode]: '' }
  void store.loadHealthFactorSources(factorCode, history.at(-1) ?? null).catch(() => {
    factorSourceErrors.value = { ...factorSourceErrors.value, [factorCode]: '来源暂时无法读取，请重试。' }
  })
}

function sourceKindLabel(kind: string) {
  return ({
    'volume-plan': '卷级规划',
    task: '卷内任务',
    obligation: '剧情义务',
    foreshadow: '伏笔',
    'reader-desire': '读者期待',
  } as Record<string, string>)[kind] ?? '正式记录'
}

onMounted(() => { void store.loadHealth().catch(() => undefined) })

function refresh() {
  pageHistory.value = []
  void store.loadHealth(true).catch(() => undefined)
}

function nextPage() {
  const cursor = health.value?.issues.nextCursor
  if (!cursor) return
  pageHistory.value.push(cursor)
  void store.loadHealth(true, cursor).catch(() => undefined)
}

function previousPage() {
  pageHistory.value.pop()
  const cursor = pageHistory.value.at(-1) ?? null
  void store.loadHealth(true, cursor).catch(() => undefined)
}

function openDestination(destination: string) {
  const id = store.currentProjectId
  if (!id) return
  if (destination.includes('历史')) router.push(`/projects/${id}/history`)
  else if (destination.includes('创作')) router.push(`/projects/${id}/production`)
  else router.push(`/projects/${id}/volumes`)
}
</script>

<template>
  <div class="page">
    <PageHeader title="长篇健康" subtitle="依据当前正式记录，帮助你及时发现长期推进中的风险与趋势。">
      <button class="btn" :disabled="loading" :aria-busy="loading" @click="refresh">{{ loading ? '正在检查…' : '重新检查' }}</button>
    </PageHeader>

    <div v-if="loading" class="card pad section" role="status">正在检查长篇健康，请稍候。</div>
    <div v-else-if="error" class="card pad section" role="alert">
      <h2 class="card-title">暂时无法完成这一项检查</h2>
      <p class="card-subtitle">当前健康数据无法读取，请稍后重试。</p>
      <button class="btn" style="margin-top:16px" @click="refresh">再次检查</button>
    </div>
    <template v-else-if="health">
      <div class="metric-grid section">
        <MetricCard label="总体状态" :value="healthLevelLabel(health.level)" :note="`检查于 ${health.checkedAt}`" />
        <MetricCard label="健康参考分" :value="scoreText" note="仅用于了解趋势，不替代创作决定" />
        <MetricCard label="正式状态检查" :value="`${health.coverage.evaluatedFactors}/8`" note="已完成的因素检查" />
        <MetricCard label="需要关注" :value="health.issues.total" note="按风险优先级排列" />
      </div>

      <div v-if="health.level === 'not_evaluated' || health.level === 'insufficient_data'" class="banner info section">
        <div><div class="banner-title">{{ healthLevelLabel(health.level) }}</div><div class="banner-copy">正式章节与运行记录积累后，长期趋势会逐步清晰。</div></div>
      </div>

      <section class="section">
        <div class="section-head"><div><h2>八类健康因素</h2><div class="muted" style="font-size:12px">正式状态检查与创作建议分开标注。</div></div></div>
        <div class="workspace-cards">
          <article v-for="factor in health.factors" :key="factor.factorCode" class="card pad">
            <div class="card-head"><div><span class="badge" :class="factor.severity === 'risk' ? 'danger' : factor.severity === 'attention' ? 'warning' : 'success'">{{ severityLabel(factor.severity) }}</span><h3 style="margin:8px 0 4px">{{ factorLabel(factor.factorCode) }}</h3></div><small class="muted">{{ truthClassLabel(factor.truthClass) }}</small></div>
            <p class="card-subtitle">{{ factor.summary }}</p>
            <p class="muted" style="font-size:12px">{{ factor.detail }}</p>
            <p class="muted health-source-count">
              可核验来源：{{ factorSourceCount(factor) }} 项
              <span v-if="factor.sourceRefsTruncated">（当前列出前 {{ factor.sourceRefs.length }} 项，完整来源可按页查看）</span>
            </p>
            <div v-if="factorSourceCount(factor) > 0" class="health-source-control">
              <button v-if="factor.sourceRefsTruncated" class="btn small" :disabled="store.isWorkspaceLoading(`health-sources:${factor.factorCode}`)" @click="openFactorSources(factor.factorCode)">
                {{ factorSourcePages[factor.factorCode] ? '重新读取来源' : '查看来源' }}
              </button>
              <details v-else-if="factor.sourceRefs.length" class="health-source-page">
                <summary>查看来源</summary>
                <ul>
                  <li v-for="item in factor.sourceRefs" :key="`${item.kind}:${item.ref}`">
                    <span>{{ sourceKindLabel(item.kind) }}</span><code>{{ item.ref }}</code>
                  </li>
                </ul>
              </details>
              <p v-if="factorSourceErrors[factor.factorCode]" class="muted" role="alert">{{ factorSourceErrors[factor.factorCode] }}</p>
              <div v-if="factorSourcePages[factor.factorCode]" class="health-source-page">
                <ul>
                  <li v-for="item in factorSourcePages[factor.factorCode].items" :key="`${item.kind}:${item.ref}`">
                    <span>{{ sourceKindLabel(item.kind) }}</span><code>{{ item.ref }}</code>
                  </li>
                </ul>
                <div class="health-factor-foot">
                  <span class="muted">第 {{ (factorSourceCursorHistory[factor.factorCode]?.length ?? 0) * 50 + 1 }}–{{ (factorSourceCursorHistory[factor.factorCode]?.length ?? 0) * 50 + factorSourcePages[factor.factorCode].items.length }} 项，共 {{ factorSourcePages[factor.factorCode].total }} 项</span>
                  <div class="actions">
                    <button class="btn small" :disabled="!(factorSourceCursorHistory[factor.factorCode]?.length) || store.isWorkspaceLoading(`health-sources:${factor.factorCode}`)" @click="previousFactorSources(factor.factorCode)">上一页</button>
                    <button class="btn small" :disabled="!factorSourcePages[factor.factorCode].nextCursor || store.isWorkspaceLoading(`health-sources:${factor.factorCode}`)" @click="nextFactorSources(factor.factorCode)">下一页</button>
                  </div>
                </div>
              </div>
            </div>
            <div class="health-factor-foot"><span class="muted">{{ statusLabel(factor.status) }}</span><button class="btn small" @click="openDestination(factor.recommendedDestination)">{{ destinationLabel(factor.recommendedDestination) }}</button></div>
          </article>
        </div>
      </section>

      <section class="section">
        <div class="section-head"><div><h2>需要关注</h2><div class="muted" style="font-size:12px">只显示当前页；数据来源位置已转换为创作语言。</div></div></div>
        <div class="filter-row">
          <label>因素<select v-model="factorFilter"><option v-for="option in factorOptions" :key="option">{{ option }}</option></select></label>
          <label>状态<select v-model="severityFilter"><option>全部状态</option><option>优先处理</option><option>需要关注</option><option>稳定</option></select></label>
        </div>
        <div v-if="filteredIssues.length" style="display:grid;gap:10px">
          <article v-for="issue in filteredIssues" :key="issueKey(issue)" class="card pad">
            <div class="card-head"><div><span class="badge" :class="issue.severity === 'risk' ? 'danger' : issue.severity === 'attention' ? 'warning' : 'info'">{{ factorLabel(issue.factorCode) }} · {{ severityLabel(issue.severity) }}</span><h3 style="margin:8px 0 4px">{{ issue.title }}</h3><p class="muted">{{ issue.detail }}</p></div><button class="btn small" @click="openDestination(issue.recommendedDestination)">{{ destinationLabel(issue.recommendedDestination) }}</button></div>
            <div class="banner info"><div><div class="banner-title">相关位置</div><div class="banner-copy">{{ healthIssueLocation(issue) }}<span v-if="issue.dueChapter"> · 预定第 {{ issue.dueChapter }} 章</span></div></div></div>
          </article>
        </div>
        <div v-else class="empty">当前筛选下没有需要处理的事项。</div>
        <div class="section-head" style="margin-top:14px"><span class="muted">共 {{ health.issues.total }} 项</span><div class="actions"><button class="btn small" :disabled="!pageHistory.length || loading" @click="previousPage">上一页</button><button class="btn small" :disabled="!health.issues.nextCursor || loading" @click="nextPage">下一页</button></div></div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.health-factor-foot { display:flex; align-items:center; justify-content:space-between; gap:10px; margin-top:14px; }
.health-source-control { display:grid; gap:8px; margin-top:10px; }
.health-source-page { display:grid; gap:8px; padding:10px; border:1px solid var(--line); border-radius:8px; }
.health-source-page ul { display:grid; gap:5px; margin:0; padding-left:18px; color:var(--ink-3); font-size:12px; }
.health-source-page li { display:flex; flex-wrap:wrap; gap:8px; align-items:center; }
.health-source-page code { color:var(--ink-2); overflow-wrap:anywhere; }
.filter-row { display:flex; flex-wrap:wrap; gap:12px; margin-bottom:14px; }
.filter-row label { display:grid; gap:4px; color:var(--ink-3); font-size:12px; }
.filter-row select { min-width:150px; padding:7px 10px; border:1px solid var(--line); border-radius:7px; background:var(--surface); }
</style>
