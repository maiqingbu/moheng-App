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
const idea = ref('一个少年带着一柄来历不明的断剑来到寒江剑派。他想寻找失踪的兄长，却逐渐发现这个江湖里“名字”本身可能被人篡改。')
const mode = ref<'AUTO'|'DIRECTOR'>('DIRECTOR')
const platform = ref('AI 推荐')
const length = ref('120–180 万字')
const forbidden = ref('不要悲剧结局；不要后宫；避免主角长期被动；不要靠低智误会推进剧情。')
const selectedTags = ref(['武侠','悬疑','成长'])
const selectedMarket = ref('market-growth')
const entryMode = ref<'idea' | 'market'>('idea')
const marketSource = ref('')
const marketMaterial = ref('')
const showInference = ref(false)
const showMarket = ref(false)
const createKey = 'project-create'
const marketKey = 'market-refresh'
const clarifyAnswers = ref({ priority:'悬疑感更强', protagonist:'可以受挫，但关键节点必须主动选择', relation:'师徒关系重要，但不要压过主线' })

const market = computed(() => store.market)
onMounted(() => store.loadMarket())
const platforms = ['AI 推荐','番茄小说','起点中文网','晋江文学城','其他']
const tags = ['武侠','悬疑','成长','志怪','群像','复仇','轻喜','慢热']
const createState = computed(()=>store.commandState(createKey))
const marketState = computed(()=>store.commandState(marketKey))
const inference = computed(() => [
  ['目标平台', platform.value === 'AI 推荐' ? '番茄小说（AI 推荐）' : platform.value],
  ['题材方向', selectedTags.value.join(' · ') || '由 AI 推断'],
  ['目标篇幅', length.value],
  ['创作模式', mode.value === 'AUTO' ? '全自动模式' : '导演模式'],
  ['内容禁区', forbidden.value || '未设置']
])

function toggleTag(tag:string){
  selectedTags.value = selectedTags.value.includes(tag) ? selectedTags.value.filter(t=>t!==tag) : [...selectedTags.value,tag].slice(0,4)
}

function startPitch(){ entryMode.value = 'idea'; showInference.value = true }
async function createFromIdea(){
  const receipt = await store.runCommand(createKey,{type:'project_create_from_idea',payload:{title:projectTitle.value,idea:idea.value,mode:mode.value,platform:platform.value,length:length.value,forbidden:forbidden.value,tags:selectedTags.value,clarifications:clarifyAnswers.value}})
  if (receipt.finalStatus==='committed' && receipt.projectId) { showInference.value=false; router.push(`/projects/${receipt.projectId}/directions`) }
}
async function refreshMarket(){ await store.runCommand(marketKey,{type:'market_refresh'}); await store.loadMarket() }
async function createFromMarket(){
  const receipt = await store.runCommand(createKey,{type:'project_create_from_market',payload:{title:projectTitle.value,marketSource:marketSource.value,marketMaterial:marketMaterial.value,mode:mode.value,forbidden:forbidden.value}})
  if (receipt.finalStatus==='committed' && receipt.projectId) { showMarket.value=false; router.push(`/projects/${receipt.projectId}/directions`) }
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
            <button class="btn soft" @click="entryMode='market';showMarket=true">从市场机会开始</button>
            <span class="muted" style="font-size:11px">支持一句话、梗概、人物设想、旧稿片段</span>
          </div>
          <button class="btn primary large" :disabled="!idea.trim()" @click="startPitch">生成三份故事方向</button>
        </div>
      </div>
      <div class="entry-paths" style="display:flex;gap:12px;margin-top:14px;flex-wrap:wrap">
        <button class="btn primary" @click="entryMode='idea';showInference=true">我已经有故事想法</button>
        <button class="btn soft" @click="entryMode='market';showMarket=true">帮我找值得写的方向</button>
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
      <div class="section-head"><div><h2>市场机会 · 本周趋势</h2><div class="muted" style="font-size:12px">市场用于发现需求，不复制任何具体作品情节、人物或文风。</div></div><div class="actions"><button class="btn small" @click="refreshMarket">重新分析</button><button class="btn small" @click="showMarket=true">查看完整市场分析</button></div></div><CommandFeedback :state="marketState" />
      <div class="market-grid">
        <article v-for="item in market" :key="item.id" class="market-card" :class="{selected:selectedMarket===item.id}" @click="selectedMarket=item.id">
          <div style="display:flex;justify-content:space-between;gap:8px"><span class="badge" :class="item.kind==='稳健型'?'pine':item.kind==='潜力型'?'info':'warning'">{{ item.kind }}</span><span class="muted" style="font-size:10px">{{ item.collectedAt }}</span></div>
          <h3>{{ item.title }}</h3><div class="market-meta">{{ item.platform }} · {{ item.direction }}</div>
          <div class="score-grid"><div class="score"><b>{{ item.demand }}</b><span>需求强度</span></div><div class="score"><b>{{ item.growth }}</b><span>增长趋势</span></div><div class="score"><b>{{ item.originalitySpace }}</b><span>原创空间</span></div></div>
          <p class="muted" style="font-size:12px;min-height:58px">{{ item.recommendWhy }}</p>
          <div class="confidence"><span>数据置信度 <b>{{ item.dataConfidence }}%</b></span><span>推荐置信度 <b>{{ item.recommendationConfidence }}%</b></span></div>
          <button class="btn small" :class="selectedMarket===item.id?'primary':'soft'" style="width:100%;margin-top:12px" @click.stop="selectedMarket=item.id">{{ selectedMarket===item.id?'已选择此机会':'按此方向创作' }}</button>
        </article>
      </div>
    </section>

    <BaseModal :open="showInference" title="确认创作方向" subtitle="这些是系统根据你的输入整理出的正式创作约束。" @close="showInference=false">
      <div class="inference-list"><div v-for="row in inference" :key="row[0]" class="inference-row"><span class="muted">{{ row[0] }}</span><strong style="max-width:66%;text-align:right">{{ row[1] }}</strong></div></div>
      <div v-if="mode==='DIRECTOR'" class="section" style="margin-top:16px"><h3 style="font-size:13px">三个高价值澄清问题</h3><div style="display:grid;gap:10px"><label class="field"><span class="field-label">更在意哪种阅读体验？</span><select v-model="clarifyAnswers.priority" class="select"><option>悬疑感更强</option><option>成长爽感更强</option><option>人物关系更强</option></select></label><label class="field"><span class="field-label">主角可以被压到什么程度？</span><input v-model="clarifyAnswers.protagonist" class="input" /></label><label class="field"><span class="field-label">人物关系希望占多大比重？</span><input v-model="clarifyAnswers.relation" class="input" /></label></div></div><div class="banner info" style="margin-top:16px"><div><div class="banner-title">下一步会生成三份实质不同的故事方向</div><div class="banner-copy">分别偏向忠于原始想法、商业增强和差异化，不会只是改名字。</div></div></div>
      <CommandFeedback :state="createState" /><template #footer><button class="btn" @click="showInference=false">继续修改</button><button class="btn primary" :disabled="['submitting','accepted','processing'].includes(createState.status)" @click="createFromIdea">确认并生成方向</button></template>
    </BaseModal>

    <BaseModal :open="showMarket" title="市场机会分析" subtitle="来源、日期与置信度都必须可追溯；低置信度数据不会冒充确定结论。" @close="showMarket=false">
      <div style="display:grid;gap:12px">
        <label class="field"><span class="field-label">资料来源</span><input v-model="marketSource" class="input" placeholder="例如：我整理的读者评论、平台观察或访谈记录" /></label>
        <label class="field"><span class="field-label">粘贴你的市场资料</span><textarea v-model="marketMaterial" class="textarea" style="min-height:120px" placeholder="粘贴一份你有权使用的市场观察；资料只用于机会判断，不会直接写入作品事实。" /></label>
        <div v-if="!market.length" class="empty">尚未配置市场资料源；粘贴一份你的市场观察即可分析。</div>
        <article v-for="item in market" :key="item.id" class="card pad" :style="selectedMarket===item.id?'border-color:var(--pine-400)':''">
          <div style="display:flex;justify-content:space-between;gap:10px"><div><span class="badge pine">{{ item.kind }}</span><h3 style="margin:8px 0 2px">{{ item.title }}</h3><div class="muted" style="font-size:11px">{{ item.source }} · {{ item.collectedAt }} · 政策 {{ item.policyVersion }}</div></div><button class="btn small" :class="selectedMarket===item.id?'primary':''" @click="selectedMarket=item.id">{{ selectedMarket===item.id?'已选':'选择' }}</button></div>
          <div class="score-grid" style="grid-template-columns:repeat(3,1fr);margin-top:14px"><div class="score"><b>{{ item.demand }}</b><span>需求</span></div><div class="score"><b>{{ item.competition }}</b><span>竞争</span></div><div class="score"><b>{{ item.homogeneityRisk }}</b><span>同质化</span></div><div class="score"><b>{{ item.growth }}</b><span>增长</span></div><div class="score"><b>{{ item.monetization }}</b><span>变现</span></div><div class="score"><b>{{ item.newcomerDifficulty }}</b><span>新作者难度</span></div><div class="score"><b>{{ item.longFormCapacity }}</b><span>长篇容量</span></div><div class="score"><b>{{ item.originalitySpace }}</b><span>原创空间</span></div><div class="score"><b>{{ item.aiFeasibility }}</b><span>AI 可行性</span></div></div><p style="margin:12px 0 6px"><strong>为什么推荐：</strong>{{ item.recommendWhy }}</p><p class="muted" style="font-size:12px"><strong>风险：</strong>{{ item.notRecommendWhy }}</p>
          <div class="chip-row"><span v-for="e in item.evidence" :key="e" class="badge">{{ e }}</span></div>
        </article>
      </div>
      <CommandFeedback :state="createState" /><template #footer><button class="btn" @click="showMarket=false">关闭</button><button class="btn primary" :disabled="!marketSource.trim() || !marketMaterial.trim() || ['submitting','accepted','processing'].includes(createState.status)" @click="createFromMarket">分析这份市场资料</button></template>
    </BaseModal>
  </div>
</template>
