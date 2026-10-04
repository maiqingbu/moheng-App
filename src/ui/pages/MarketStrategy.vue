<script setup lang="ts">
import { computed, onMounted } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import type { MarketSourceStatus } from '../../application/contracts'

const store = useProjectStore()
const strategy = computed(() => store.marketStrategy)
const refreshKey = 'market-refresh'
const selectKey = 'market-opportunity-select'
const refreshState = computed(() => store.commandState(refreshKey))
const selectState = computed(() => store.commandState(selectKey))
const busy = computed(() => store.isWorkspaceLoading('market') || ['submitting', 'accepted', 'processing'].includes(refreshState.value.status) || ['submitting', 'accepted', 'processing'].includes(selectState.value.status))

onMounted(() => void store.loadMarket())

function statusLabel(status: MarketSourceStatus | 'INDEXED') {
  const labels: Record<MarketSourceStatus | 'INDEXED', string> = {
    READY: '来源可用',
    PARTIAL: '部分可用',
    SOURCE_UNAVAILABLE: '来源暂不可用',
    STALE: '资料已过期',
    NOT_CONFIGURED: '尚未配置',
    USER_DATA_ONLY: '仅有用户资料',
    DEMO_ONLY: '仅供演示',
    INDEXED: '规则来源已索引'
  }
  return labels[status]
}

function observedTime(epochMs: number | null) {
  return epochMs !== null && Number.isFinite(epochMs) && epochMs > 0
    ? new Date(epochMs).toLocaleString('zh-CN')
    : '时间未知'
}

function metric(value: number | null) {
  return value == null ? '暂无可核验数据' : String(value)
}

async function refreshMarket() {
  if (!store.currentProjectId || busy.value) return
  try {
    await store.runCommand(refreshKey, {
      type: 'market_refresh',
      projectId: store.currentProjectId,
      payload: {}
    })
  } catch {
    // CommandFeedback presents the formal command state.
  }
}

async function selectOpportunity(marketOpportunityId: string) {
  if (!store.currentProjectId || busy.value) return
  try {
    await store.runCommand(selectKey, {
      type: 'market_opportunity_select',
      projectId: store.currentProjectId,
      payload: { marketOpportunityId }
    })
  } catch {
    // CommandFeedback presents the formal command state.
  }
}
</script>

<template>
  <div class="page">
    <PageHeader title="市场与平台策略" subtitle="官方展示样本和平台规则索引只用于机会判断；它们不会替你选择故事方向。" />

    <div v-if="!strategy && store.isWorkspaceLoading('market')" class="empty" role="status">正在读取已提交的市场资料…</div>
    <div v-else-if="!strategy" class="banner warning" role="status">
      <div><div class="banner-title">市场资料尚未读取</div><div class="banner-copy">重新读取项目记录后再试；不会用演示数据填充正式策略。</div></div>
      <button class="btn small" :disabled="busy" @click="store.loadMarket(true)">重试</button>
    </div>

    <template v-else>
      <section class="card pad section">
        <div class="section-head">
          <div>
            <div class="card-head"><h2 class="card-title">来源状态</h2><span class="badge" :class="strategy.status === 'READY' ? 'pine' : 'info'">{{ statusLabel(strategy.status) }}</span></div>
            <p class="card-subtitle">市场：{{ statusLabel(strategy.sourceStatus) }} · 规则索引：{{ statusLabel(strategy.policyStatus) }}</p>
          </div>
          <button class="btn small" :disabled="busy" @click="refreshMarket">{{ busy ? '正在处理…' : '刷新官方来源' }}</button>
        </div>
        <div v-if="strategy.source" class="muted" style="font-size:12px">
          <a :href="strategy.source.sourceUrl" target="_blank" rel="noreferrer">{{ strategy.source.sourceId }} 官方来源</a>
          · 观察于 {{ observedTime(strategy.source.observedAtEpochMs) }}
          · 页面展示 {{ strategy.source.sampleCount }} 个样本
          <span v-if="strategy.source.digest"> · 摘要 {{ strategy.source.digest }}</span>
        </div>
        <div v-else class="muted" style="font-size:12px">本项目还没有可引用的官方来源快照。</div>
        <CommandFeedback :state="refreshState" />
        <CommandFeedback :state="selectState" />
      </section>

      <div v-if="strategy.status === 'SOURCE_UNAVAILABLE'" class="banner warning section" role="status">
        <div><div class="banner-title">官方来源暂不可用</div><div class="banner-copy">当前状态不会沿用旧资料伪装成最新数据。刷新会记录一次来源尝试。</div></div>
      </div>
      <div v-else-if="strategy.status === 'STALE'" class="banner warning section" role="status">
        <div><div class="banner-title">市场资料已过期</div><div class="banner-copy">刷新并重新核验后，才能把样本用于当前机会选择。</div></div>
      </div>
      <div v-else-if="strategy.status === 'PARTIAL'" class="banner info section" role="status">
        <div><div class="banner-title">规则来源索引不完整</div><div class="banner-copy">机会样本可供观察，但这里不对平台规则作结论；请打开官方规则原文核对。</div></div>
      </div>
      <div v-else-if="strategy.status === 'DEMO_ONLY'" class="banner info section" role="status">
        <div><div class="banner-title">当前内容仅供演示</div><div class="banner-copy">演示适配器数据不会成为正式作品的市场来源或策略证据。</div></div>
      </div>

      <div class="metric-grid section">
        <article class="metric-card"><span class="muted">目标平台</span><strong>{{ strategy.targetPlatform || '未指定' }}</strong><small>{{ strategy.platformPolicyVersion ? `规则摘要 ${strategy.platformPolicyVersion}` : '未取得规则版本' }}</small></article>
        <article class="metric-card"><span class="muted">策略摘要</span><strong>{{ strategy.strategyTitle }}</strong><small>{{ strategy.readerPromise }}</small></article>
        <article class="metric-card"><span class="muted">最近观察</span><strong>{{ observedTime(strategy.snapshotCollectedAtEpochMs) }}</strong><small>下次复核：{{ strategy.nextReview }}</small></article>
      </div>

      <section class="section">
        <div class="section-head"><div><h2>官方出版展示样本</h2><p class="muted" style="font-size:12px">展示样本不代表销量、阅读量、收入、市场份额或题材趋势。</p></div></div>
        <div v-if="strategy.opportunities.length" class="market-grid">
          <article v-for="item in strategy.opportunities" :key="item.id" class="market-card">
            <span class="badge">{{ item.kind }}</span>
            <h3>{{ item.title }}</h3>
            <div class="market-meta">{{ item.platform }} · {{ item.direction }}</div>
            <div class="score-grid">
              <div class="score"><b>{{ metric(item.demand) }}</b><span>需求</span></div>
              <div class="score"><b>{{ metric(item.growth) }}</b><span>增长</span></div>
              <div class="score"><b>{{ metric(item.originalitySpace) }}</b><span>原创空间</span></div>
            </div>
            <p>{{ item.recommendWhy }}</p>
            <small class="muted">{{ item.source }} · {{ item.collectedAt }}</small>
            <div v-if="item.stale || strategy.sourceStatus !== 'READY'" class="banner warning" style="margin-top:12px">来源过期或不可用，当前不可选择。</div>
            <div v-else-if="strategy.selectedOpportunityId === (item.topicId ?? item.id)" class="banner info" style="margin-top:12px">已记录为本作品的市场参考；故事方向未更改。</div>
            <button
              v-else
              class="btn small soft"
              style="width:100%;margin-top:12px"
              :disabled="busy || item.metricsBasis !== 'OFFICIAL_PUBLICATION_SAMPLE'"
              @click="selectOpportunity(item.topicId ?? item.id)"
            >明确选择为市场参考</button>
          </article>
        </div>
        <div v-else class="empty">当前没有可用机会样本。请刷新来源，或继续依据自己的资料判断。</div>
      </section>

      <section v-if="strategy.policyReferences.length" class="card pad section">
        <h2 class="card-title">官方规则来源索引</h2>
        <p class="card-subtitle">此处只提供官方页面入口，不代替阅读和核验规则原文。</p>
        <ul><li v-for="reference in strategy.policyReferences" :key="reference.url"><a :href="reference.url" target="_blank" rel="noreferrer">{{ reference.title }}</a><span v-if="reference.publishedAt" class="muted"> · {{ reference.publishedAt }}</span></li></ul>
      </section>

      <section class="card pad section">
        <h2 class="card-title">使用边界</h2>
        <ul><li v-for="guardrail in strategy.guardrails" :key="guardrail">{{ guardrail }}</li></ul>
      </section>
    </template>
  </div>
</template>
