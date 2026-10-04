<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import BaseModal from '../components/BaseModal.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectLibraryStore } from '../stores/projectLibraryStore'

const router = useRouter()
const store = useProjectLibraryStore()
const projectTitle = ref('')
const idea = ref('')
const mode = ref<'AUTO'|'DIRECTOR'>('DIRECTOR')
const platform = ref('待判断')
const length = ref('AI 推荐')
const forbidden = ref('')
const selectedTags = ref<string[]>([])
const selectedMarket = ref('')
const marketSource = ref('')
const marketMaterial = ref('')
const showMarket = ref(false)
const createKey = 'project-create'

const marketDiscovery = computed(() => store.market)
const market = computed(() => store.market.opportunities)
onMounted(() => store.loadMarket())
const platforms = ['待判断','番茄小说','起点中文网','晋江文学城','其他']
const tags = ['武侠','悬疑','成长','志怪','群像','复仇','轻喜','慢热']
const createState = computed(()=>store.commandState(createKey))
const createInFlight = computed(() => ['submitting','accepted','processing'].includes(createState.value.status))
const inference = computed(() => [
  ['目标平台', platform.value],
  ['题材方向', selectedTags.value.join(' · ') || '待判断'],
  ['目标篇幅', length.value],
  ['创作模式', mode.value === 'AUTO' ? '全自动模式' : '导演模式'],
  ['内容禁区', forbidden.value || '未设置']
])

function toggleTag(tag:string){
  selectedTags.value = selectedTags.value.includes(tag) ? selectedTags.value.filter(t=>t!==tag) : [...selectedTags.value,tag].slice(0,4)
}

function marketMetric(value: number | null) {
  return value == null ? '—' : value
}

function marketConfidence(value: number | null) {
  return value == null ? '暂不可评估' : `${value}%`
}

function canCreateFromMarket() {
  const selected = market.value.find(item => item.id === selectedMarket.value)
  const selectedOfficial = selected
    && ['READY', 'PARTIAL'].includes(marketDiscovery.value.status)
    && selected.metricsBasis === 'OFFICIAL_PUBLICATION_SAMPLE'
    && selected.stale !== true
  const manualMaterial = marketSource.value.trim().length > 0 && marketMaterial.value.trim().length > 0
  return Boolean(selectedOfficial || manualMaterial)
}

const selectedMarketItem = computed(() => market.value.find(item => item.id === selectedMarket.value) ?? null)

async function createFromIdea(){
  if (createInFlight.value || !idea.value.trim()) return
  try {
    const receipt = await store.runCommand(createKey,{type:'project_create_from_idea',payload:{title:projectTitle.value,idea:idea.value,mode:mode.value,platform:platform.value,length:length.value,forbidden:forbidden.value,tags:selectedTags.value}})
    if (receipt.finalStatus==='committed' && receipt.projectId) router.push(`/projects/${receipt.projectId}/directions?generate=1`)
  } catch (error) {
    console.error('[project-entry] project creation failed', error)
  }
}
async function createFromMarket(){
  if (createInFlight.value || !canCreateFromMarket()) return
  const selected = market.value.find(item => item.id === selectedMarket.value)
  const useOfficialSource = selected
    && ['READY', 'PARTIAL'].includes(marketDiscovery.value.status)
    && selected.metricsBasis === 'OFFICIAL_PUBLICATION_SAMPLE'
    && selected.stale !== true
  const payload: Record<string, unknown> = {
    title: projectTitle.value,
    mode: mode.value,
    platform: platform.value,
    length: length.value,
    forbidden: forbidden.value,
    tags: selectedTags.value
  }
  if (useOfficialSource && selected) {
    payload.marketOpportunityId = selected.id
  } else {
    payload.marketSource = marketSource.value.trim()
    payload.marketMaterial = marketMaterial.value.trim()
  }
  try {
    const receipt = await store.runCommand(createKey,{type:'project_create_from_market',payload})
    if (receipt.finalStatus==='committed' && receipt.projectId) { showMarket.value=false; router.push(`/projects/${receipt.projectId}/directions?generate=1`) }
  } catch {
    // CommandFeedback keeps the typed provider failure visible in the modal.
  }
}
</script>

<template>
  <div class="page">
    <PageHeader title="新建作品" subtitle="你只需要说清楚想看什么、不要什么；系统负责把意图转化为可生产的小说项目。" />

    <section class="hero">
      <div class="hero-eyebrow">长篇小说创作</div>
      <h1>从一个念头，<br>到一部可以持续生产的小说。</h1>
      <p>不需要先写大纲，也不需要理解提示词。描述一个念头、人物、场景或你喜欢的阅读体验，系统会完成市场判断、故事立项、设定、开篇审核与后续生产。</p>
      <div class="idea-composer">
        <input v-model="projectTitle" class="input project-title-input" placeholder="作品名称（可稍后修改）" aria-label="作品名称" />
        <textarea v-model="idea" aria-label="故事想法" placeholder="例如：我想看一个不愿当英雄的人，被迫卷进一场关于名字与记忆的江湖阴谋……" />
        <div class="composer-foot">
          <div class="actions">
            <button class="btn soft" @click="showMarket=true">从市场机会开始</button>
            <span class="muted" style="font-size:11px">支持一句话、梗概、人物设想、旧稿片段</span>
          </div>
          <button class="btn primary large" :aria-disabled="!idea.trim() || createInFlight" @click="createFromIdea">生成三份故事方向</button>
        </div>
      </div>
      <CommandFeedback :state="createState" />
      <div class="entry-paths" style="display:flex;gap:12px;margin-top:14px;flex-wrap:wrap">
        <button class="btn primary" :aria-disabled="!idea.trim() || createInFlight" @click="createFromIdea">我已经有故事想法</button>
        <button class="btn soft" @click="showMarket=true">帮我找值得写的方向</button>
      </div>
    </section>

    <div class="launch-grid">
      <section class="card pad">
        <div class="card-head"><div><h2 class="card-title">创作偏好</h2><p class="card-subtitle">只保留真正会影响故事方向的少量输入。</p></div></div>
        <div style="display:grid;gap:16px">
          <div class="field">
            <span class="field-label">创作模式</span>
            <div class="segmented"><button :class="{active:mode==='AUTO'}" @click="mode='AUTO'">全自动</button><button :class="{active:mode==='DIRECTOR'}" @click="mode='DIRECTOR'">导演模式</button></div>
            <span class="help">全自动模式会尽量减少询问；导演模式只在关键选择点请求你的决定。</span>
          </div>
          <div class="setting-grid">
            <label class="field"><span class="field-label">目标平台</span><select v-model="platform" class="select"><option v-for="p in platforms" :key="p">{{ p }}</option></select></label>
            <label class="field"><span class="field-label">期望篇幅</span><select v-model="length" class="select"><option>AI 推荐</option><option>40–80 万字</option><option>80–120 万字</option><option>120–180 万字</option><option>180 万字以上</option></select></label>
          </div>
          <div class="field"><span class="field-label">偏好的阅读体验</span><div class="chip-row"><button v-for="tag in tags" :key="tag" class="chip" :class="{active:selectedTags.includes(tag)}" @click="toggleTag(tag)">{{ tag }}</button></div></div>
          <label class="field"><span class="field-label">不希望出现的内容</span><textarea v-model="forbidden" class="textarea" style="min-height:88px" placeholder="例如：不要悲剧、不要后宫、不要长期虐主……" /><span class="help">这些限制会进入正式创作约束，而不是只作为聊天偏好。</span></label>
        </div>
      </section>

      <section class="card pad">
        <div class="card-head"><div><h2 class="card-title">系统整理的创作约束</h2><p class="card-subtitle">全自动模式可以直接采用；你随时可以修改。</p></div><span class="badge pine">可修改</span></div>
        <div class="inference-list">
          <div v-for="row in inference" :key="row[0]" class="inference-row"><span class="muted">{{ row[0] }}</span><strong style="max-width:64%;text-align:right;font-size:12px">{{ row[1] }}</strong></div>
        </div>
      </section>
    </div>

    <section class="section">
      <div class="section-head"><div><h2>官方市场来源观察</h2><div class="muted" style="font-size:12px">仅汇总番茄作家专区公开出版展示中的分类样本，不含阅读量、销量或排名。</div></div><div class="actions"><button class="btn small" @click="showMarket=true">查看来源与手动资料</button><button class="btn small" :disabled="store.marketLoading" @click="store.loadMarket(true)">{{ store.marketLoading ? '正在刷新…' : '刷新来源' }}</button></div></div>
      <div v-if="marketDiscovery.status==='SOURCE_UNAVAILABLE'" class="banner warning"><div><div class="banner-title">官方市场来源暂不可用</div><div class="banner-copy">不会用旧资料冒充当前状态。你可以重试，或改用自己有权使用的市场观察。</div></div><button class="btn small" :disabled="store.marketLoading" @click="store.loadMarket(true)">重试</button></div>
      <div v-else-if="marketDiscovery.status==='PARTIAL'" class="banner warning"><div><div class="banner-title">市场资料部分可用</div><div class="banner-copy">平台规则来源索引暂不可用，机会样本不包含平台规则结论。</div></div></div>
      <div v-if="market.length" class="market-grid">
        <article v-for="item in market" :key="item.id" class="market-card" :class="{selected:selectedMarket===item.id}">
          <div style="display:flex;justify-content:space-between;gap:8px"><span class="badge info">{{ item.kind }}</span><span class="muted" style="font-size:10px">样本 {{ item.sampleCount ?? marketDiscovery.source?.sampleCount ?? 0 }}</span></div>
          <h3>{{ item.title }}</h3><div class="market-meta">{{ item.platform }} · {{ item.direction }}</div>
          <div class="score-grid"><div class="score"><b>{{ marketMetric(item.demand) }}</b><span>需求</span></div><div class="score"><b>{{ marketMetric(item.growth) }}</b><span>增长</span></div><div class="score"><b>{{ marketMetric(item.originalitySpace) }}</b><span>原创空间</span></div></div>
          <p class="muted" style="font-size:12px;min-height:58px">{{ item.recommendWhy }}</p>
          <div class="confidence"><span>来源置信度 <b>{{ marketConfidence(item.dataConfidence) }}</b></span><span>机会建议 <b>{{ marketConfidence(item.recommendationConfidence) }}</b></span></div>
          <button class="btn small" :class="selectedMarket===item.id?'primary':'soft'" style="width:100%;margin-top:12px" @click="selectedMarket=item.id">{{ selectedMarket===item.id?'已选择此样本':'选择为立项参考' }}</button>
        </article>
      </div>
      <div v-else-if="store.marketLoading" class="empty">正在读取官方公开市场来源…</div>
      <div v-else-if="marketDiscovery.status==='DEMO_ONLY'" class="empty">当前显示的是浏览器演示数据，不会进入正式作品来源链。</div>
      <div v-else class="empty">尚无可用官方展示样本。你可以在“查看来源与手动资料”中提供自己有权使用的观察资料。</div>
      <div v-if="selectedMarketItem" class="banner info" style="margin-top:12px"><div><div class="banner-title">已选作立项参考</div><div class="banner-copy">正式提交时系统会重新验证来源并记录快照；这项选择不会直接改写作品事实或故事方向。</div></div></div>
    </section>

    <BaseModal :open="showMarket" title="市场机会分析" subtitle="来源、日期与置信度都必须可追溯；低置信度数据不会冒充确定结论。" @close="showMarket=false">
      <div style="display:grid;gap:12px">
        <div v-if="marketDiscovery.source" class="card pad"><div class="muted">来源：{{ marketDiscovery.source.sourceId }} · 观察时间 {{ new Date(marketDiscovery.source.observedAtEpochMs).toLocaleString('zh-CN') }}</div><div v-if="marketDiscovery.source.digest" class="muted" style="font-size:11px">来源摘要：{{ marketDiscovery.source.digest }}</div><div class="muted" style="font-size:11px;margin-top:6px">{{ marketDiscovery.limitations.join(' ') }}</div></div>
        <label class="field"><span class="field-label">资料来源（手动资料时填写）</span><input v-model="marketSource" class="input" placeholder="例如：我整理的读者评论、平台观察或访谈记录" /></label>
        <label class="field"><span class="field-label">补充市场观察（可选）</span><textarea v-model="marketMaterial" class="textarea" style="min-height:120px" placeholder="只粘贴你有权使用的资料；外部文本按数据处理，不作为系统或作品规则。" /></label>
        <div v-if="marketDiscovery.policyReferences.length" class="card pad"><strong>官方规则来源索引</strong><div v-for="reference in marketDiscovery.policyReferences" :key="reference.url" class="muted" style="margin-top:6px"><a :href="reference.url" target="_blank" rel="noreferrer">{{ reference.title }}</a><span v-if="reference.publishedAt"> · {{ reference.publishedAt }}</span></div></div>
        <div v-if="!market.length && marketDiscovery.status!=='SOURCE_UNAVAILABLE'" class="empty">暂无可选择机会；可继续使用你有权使用的手动市场资料。</div>
        <article v-for="item in market" :key="item.id" class="card pad" :style="selectedMarket===item.id?'border-color:var(--pine-400)':''">
          <div style="display:flex;justify-content:space-between;gap:10px"><div><span class="badge pine">{{ item.kind }}</span><h3 style="margin:8px 0 2px">{{ item.title }}</h3><div class="muted" style="font-size:11px">{{ item.source }} · {{ item.collectedAt }} · 政策 {{ item.policyVersion }}</div></div><button class="btn small" :class="selectedMarket===item.id?'primary':''" @click="selectedMarket=item.id">{{ selectedMarket===item.id?'已选':'选择' }}</button></div>
          <div class="score-grid" style="grid-template-columns:repeat(3,1fr);margin-top:14px"><div class="score"><b>{{ marketMetric(item.demand) }}</b><span>需求</span></div><div class="score"><b>{{ marketMetric(item.competition) }}</b><span>竞争</span></div><div class="score"><b>{{ marketMetric(item.homogeneityRisk) }}</b><span>同质化</span></div><div class="score"><b>{{ marketMetric(item.growth) }}</b><span>增长</span></div><div class="score"><b>{{ marketMetric(item.monetization) }}</b><span>变现</span></div><div class="score"><b>{{ marketMetric(item.newcomerDifficulty) }}</b><span>新作者难度</span></div><div class="score"><b>{{ marketMetric(item.longFormCapacity) }}</b><span>长篇容量</span></div><div class="score"><b>{{ marketMetric(item.originalitySpace) }}</b><span>原创空间</span></div><div class="score"><b>{{ marketMetric(item.aiFeasibility) }}</b><span>AI 可行性</span></div></div><p style="margin:12px 0 6px"><strong>观察说明：</strong>{{ item.recommendWhy }}</p><p class="muted" style="font-size:12px"><strong>限制：</strong>{{ item.notRecommendWhy }}</p>
          <div class="chip-row"><span v-for="e in item.evidence" :key="e" class="badge">{{ e }}</span></div>
        </article>
      </div>
      <CommandFeedback :state="createState" /><template #footer><button class="btn" @click="showMarket=false">关闭</button><button class="btn primary" :disabled="createInFlight || !canCreateFromMarket()" :aria-disabled="createInFlight || !canCreateFromMarket()" @click="createFromMarket">{{ selectedMarketItem ? '按已选样本继续立项' : '分析这份手动市场资料' }}</button></template>
    </BaseModal>
  </div>
</template>
