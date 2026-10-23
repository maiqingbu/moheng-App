<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import type { CommandType } from '../../application/contracts'
import {
  bibleProblems,
  bibleSummary,
  craftConfirmRequest,
  linesToList,
  listToLines,
  sampleReadiness,
  storyBibleGenerateRequest,
  styleCardGenerateRequest,
  styleSummary,
  TARGET_LABELS,
} from '../presentation/craftSetup'

type Json = Record<string, any>

const store = useProjectStore()
const router = useRouter()
const setup = computed(() => store.craftSetup)
const loadError = ref('')
const projectPath = (suffix: string) => `/projects/${store.currentProjectId ?? ''}/${suffix}`
const busy = (key: string) => ['submitting', 'accepted', 'processing'].includes(store.commandState(key).status)

onMounted(async () => {
  try { await store.loadCraftSetup(true) } catch { loadError.value = '故事底稿与风格卡暂时无法读取，请重新打开本页。' }
})

/** Runs a command; the store reloads this projection once it commits. */
async function run(key: string, request: { type: CommandType; payload: Json }): Promise<boolean> {
  try {
    return (await store.runCommand(key, request)).finalStatus === 'committed'
  } catch { return false /* CommandFeedback shows the outcome. */ }
}

// ---- 故事底稿 ----
const adjustment = ref('')
/** The candidate being edited and its working copy. */
const bibleEdit = ref<{ candidateId: string; draft: Json } | null>(null)
const bibleDraft = computed(() => bibleEdit.value?.draft ?? null)
const problems = computed(() => (bibleDraft.value ? bibleProblems(bibleDraft.value) : []))

function editBible(source: Json, candidateId: string) {
  bibleEdit.value = { candidateId, draft: JSON.parse(JSON.stringify(source)) }
}
function editConfirmedBible() {
  const confirmed = setup.value?.storyBible.confirmed
  if (confirmed) editBible(confirmed, confirmed.confirmedFromRef?.artifactId ?? '')
}
async function confirmBible() {
  const edit = bibleEdit.value
  if (!edit || problems.value.length || busy('story-bible-confirm')) return
  if (await run('story-bible-confirm', craftConfirmRequest('bible', edit.candidateId, edit.draft))) bibleEdit.value = null
}
const protagonistFields = [['name', '姓名'], ['personality', '性格'], ['flaw', '缺点'], ['wantsNow', '眼下想要'], ['humorStyle', '幽默方式（只写描述）'], ['innerVoiceStyle', '内心声音（只写描述）']]
const antagonistFields = [['name', '姓名'], ['position', '身份'], ['wants', '想要'], ['method', '手段']]
const castFields = [['name', '姓名'], ['role', '身份'], ['relation', '与主角关系'], ['privateAgenda', '自己的盘算'], ['speechStyle', '说话方式（只写描述）'], ['contrastWithProtagonist', '与主角的反差']]
const ruleFields = [['rule', '规则'], ['numbers', '数字与限度'], ['howReadersSeeIt', '读者看到的后果']]

// ---- 风格卡 ----
const styleSource = ref<'MARKET_ARCHETYPE' | 'USER_SAMPLE'>('MARKET_ARCHETYPE')
const primary = ref('')
const secondary = ref('')
const archetypeChoices = [{ label: '主原型', model: primary }, { label: '辅原型', model: secondary }]
const sampleText = ref('')
const readiness = computed(() => sampleReadiness(sampleText.value, setup.value?.sample?.minChars ?? 800))
/** List fields edited as one line per item. */
const LINE_FIELDS = ['openingStructure', 'rules', 'forbidden'] as const
const cardEdit = ref<{ candidateId: string; draft: Json } | null>(null)
const cardDraft = computed(() => cardEdit.value?.draft ?? null)

watch(() => setup.value?.sample?.text, text => { if (text && !sampleText.value) sampleText.value = text }, { immediate: true })

function editCard(source: Json, candidateId: string) {
  const draft = JSON.parse(JSON.stringify(source))
  for (const field of LINE_FIELDS) draft[field] = listToLines(source[field])
  cardEdit.value = { candidateId, draft }
}
function editConfirmedCard() {
  const confirmed = setup.value?.styleCard.confirmed
  if (confirmed) editCard(confirmed, confirmed.confirmedFromRef?.artifactId ?? '')
}
async function saveSample() {
  if (!readiness.value.enough || busy('style-sample-save')) return
  await run('style-sample-save', { type: 'style_sample_save', payload: { text: sampleText.value } })
}
async function generateCards() {
  if (busy('style-card-generate')) return
  await run('style-card-generate', styleSource.value === 'USER_SAMPLE'
    ? styleCardGenerateRequest('USER_SAMPLE')
    : styleCardGenerateRequest('MARKET_ARCHETYPE', primary.value || undefined, secondary.value || undefined))
}
async function confirmCard() {
  const edit = cardEdit.value
  if (!edit || busy('style-card-confirm')) return
  const card = { ...edit.draft }
  for (const field of LINE_FIELDS) card[field] = linesToList(edit.draft[field])
  if (await run('style-card-confirm', craftConfirmRequest('card', edit.candidateId, card))) cardEdit.value = null
}
</script>

<template>
  <div class="page">
    <PageHeader title="规划 · 故事底稿与风格卡" subtitle="先定下故事的底子和写法，再进入开篇准备。每样各有 3 版候选，选一版后可逐项修改。">
      <button class="btn" @click="router.push(projectPath('setup'))">返回故事核心</button>
      <button class="btn primary" @click="router.push(projectPath('opening'))">进入开篇准备</button>
    </PageHeader>

    <div v-if="loadError" class="banner warning" role="alert">{{ loadError }}</div>

    <template v-else-if="setup">
      <!-- 故事底稿 -->
      <section class="card pad section">
        <div class="card-head">
          <div><h2 class="card-title">故事底稿</h2><p class="card-subtitle">日常世界、能力规则、主角与对手、配角和前三章走向。人物只写说话方式的描述，不写现成台词。</p></div>
          <span v-if="setup.storyBible.confirmed" class="badge pine">已确认</span>
        </div>
        <div v-if="setup.storyBible.confirmed && !bibleDraft" class="banner info" style="margin-top:12px">
          <div><div class="banner-title">当前底稿：{{ bibleSummary(setup.storyBible.confirmed).title }}</div><div class="banner-copy">{{ bibleSummary(setup.storyBible.confirmed).lines.join('；') }}</div></div>
          <button class="btn small" @click="editConfirmedBible">修改</button>
        </div>
        <div class="actions" style="margin-top:16px">
          <input v-model="adjustment" class="input" placeholder="对这一批候选的要求（可不填），例如：多些人情味" />
          <button class="btn" :disabled="busy('story-bible-generate')" @click="run('story-bible-generate', storyBibleGenerateRequest(adjustment))">生成 3 版底稿</button>
        </div>
        <CommandFeedback :state="store.commandState('story-bible-generate')" @confirm="store.confirmCommandResult('story-bible-generate')" />
        <div v-if="setup.storyBible.candidates.length" class="setting-grid" style="margin-top:16px">
          <article v-for="candidate in setup.storyBible.candidates" :key="candidate.id" class="card pad">
            <h3 class="card-title">{{ bibleSummary(candidate).title }}</h3>
            <ul><li v-for="line in bibleSummary(candidate).lines" :key="line">{{ line }}</li></ul>
            <button class="btn small" @click="editBible(candidate, candidate.id)">选这一版并编辑</button>
          </article>
        </div>

        <div v-if="bibleDraft" style="display:grid;gap:16px;margin-top:20px">
          <label class="field"><span class="field-label">这一版的侧重</span><input v-model="bibleDraft.angle" class="input" /></label>
          <label class="field"><span class="field-label">日常世界（异常发生前主角的一天）</span><textarea v-model="bibleDraft.normalWorld" class="textarea" /></label>
          <label class="field"><span class="field-label">能力的体验</span><textarea v-model="bibleDraft.mechanism.experience" class="textarea" /></label>
          <div class="field"><span class="field-label">能力规则（2–5 条，带数字）</span>
            <div v-for="(rule, index) in bibleDraft.mechanism.rules" :key="index" class="setting-grid">
              <label v-for="[key, label] in ruleFields" :key="key" class="field"><span class="field-label">{{ label }}</span><input v-model="rule[key]" class="input" /></label>
              <button class="btn small" @click="bibleDraft.mechanism.rules.splice(index, 1)">删除这条</button>
            </div>
            <button class="btn small" @click="bibleDraft.mechanism.rules.push({ rule: '', numbers: '', howReadersSeeIt: '' })">加一条规则</button>
          </div>
          <div class="field"><span class="field-label">主角</span>
            <div class="setting-grid"><label v-for="[key, label] in protagonistFields" :key="key" class="field"><span class="field-label">{{ label }}</span><input v-model="bibleDraft.protagonist[key]" class="input" /></label></div>
          </div>
          <div class="field"><span class="field-label">情感锚点</span>
            <div class="setting-grid">
              <label class="field"><span class="field-label">物件</span><input v-model="bibleDraft.emotionalAnchor.object" class="input" /></label>
              <label class="field"><span class="field-label">相关的往事</span><input v-model="bibleDraft.emotionalAnchor.memorySummary" class="input" /></label>
            </div>
          </div>
          <div class="field"><span class="field-label">对手</span>
            <div class="setting-grid"><label v-for="[key, label] in antagonistFields" :key="key" class="field"><span class="field-label">{{ label }}</span><input v-model="bibleDraft.antagonist[key]" class="input" /></label></div>
          </div>
          <div class="field"><span class="field-label">配角（2–8 人）</span>
            <div v-for="(member, index) in bibleDraft.cast" :key="index" class="setting-grid">
              <label v-for="[key, label] in castFields" :key="key" class="field"><span class="field-label">{{ label }}</span><input v-model="member[key]" class="input" /></label>
              <label class="field"><span class="field-label">第几章出场</span><input v-model.number="member.firstAppears" type="number" min="1" class="input" /></label>
              <button class="btn small" @click="bibleDraft.cast.splice(index, 1)">删除此人</button>
            </div>
            <button class="btn small" @click="bibleDraft.cast.push({ name: '', role: '', relation: '', privateAgenda: '', speechStyle: '', contrastWithProtagonist: '', firstAppears: 1 })">加一位配角</button>
          </div>
          <div class="field"><span class="field-label">前三章走向</span>
            <div v-for="arc in bibleDraft.openingArc" :key="arc.chapter" class="setting-grid">
              <label class="field"><span class="field-label">第 {{ arc.chapter }} 章走向</span><input v-model="arc.direction" class="input" /></label>
              <label class="field"><span class="field-label">主角赢的一局</span><input v-model="arc.protagonistWin" class="input" /></label>
            </div>
          </div>
          <div v-if="problems.length" class="banner warning"><div><div class="banner-title">还需要修改</div><div class="banner-copy">{{ problems.join('；') }}</div></div></div>
          <CommandFeedback :state="store.commandState('story-bible-confirm')" @confirm="store.confirmCommandResult('story-bible-confirm')" />
          <div class="actions" style="justify-content:flex-end">
            <button class="btn" @click="bibleEdit = null">取消</button>
            <button class="btn primary" :disabled="problems.length > 0 || busy('story-bible-confirm')" @click="confirmBible">确认这版底稿</button>
          </div>
        </div>
      </section>

      <!-- 风格卡 -->
      <section class="card pad section">
        <div class="card-head">
          <div><h2 class="card-title">风格卡</h2><p class="card-subtitle">段落节奏、对白比例、叙述人称和写法规则。可以从市场作品整理的写法原型生成，也可以用你自己的范文测量。</p></div>
          <span v-if="setup.styleCard.confirmed" class="badge pine">已确认</span>
        </div>
        <div v-if="setup.styleCard.confirmed && !cardDraft" class="banner info" style="margin-top:12px">
          <div><div class="banner-title">当前风格卡：{{ styleSummary(setup.styleCard.confirmed).title }}</div><div class="banner-copy">{{ styleSummary(setup.styleCard.confirmed).lines.join('；') }}</div></div>
          <button class="btn small" @click="editConfirmedCard">修改</button>
        </div>
        <div class="actions" style="margin-top:16px">
          <label><input v-model="styleSource" type="radio" value="MARKET_ARCHETYPE" /> 市场写法原型</label>
          <label><input v-model="styleSource" type="radio" value="USER_SAMPLE" /> 我的范文</label>
        </div>
        <div v-if="styleSource === 'MARKET_ARCHETYPE'" class="setting-grid" style="margin-top:12px">
          <label v-for="choice in archetypeChoices" :key="choice.label" class="field"><span class="field-label">{{ choice.label }}</span>
            <select v-model="choice.model.value" class="input"><option value="">按题材推荐</option><option v-for="item in setup.archetypes" :key="item.id" :value="item.id">{{ item.name }}</option></select>
          </label>
        </div>
        <div v-else style="display:grid;gap:12px;margin-top:12px">
          <label class="field"><span class="field-label">范文（建议 2000 字左右，请使用本人作品或已获授权的文字）</span><textarea v-model="sampleText" class="textarea" rows="10" /></label>
          <div class="banner info"><div><div class="banner-title">{{ readiness.message }}</div><div class="banner-copy">范文只保存在本机，不会发给模型，也不会进入导出；风格卡里只记录测出的指标。</div></div></div>
          <div class="actions"><button class="btn" :disabled="!readiness.enough || busy('style-sample-save')" @click="saveSample">保存范文</button></div>
          <CommandFeedback :state="store.commandState('style-sample-save')" @confirm="store.confirmCommandResult('style-sample-save')" />
          <table v-if="setup.sample" class="table">
            <tbody><tr v-for="(value, key) in setup.sample.metrics" :key="key"><td>{{ TARGET_LABELS[key] ?? key }}</td><td>{{ value }}</td></tr></tbody>
          </table>
        </div>
        <div class="actions" style="margin-top:12px">
          <button class="btn" :disabled="busy('style-card-generate') || (styleSource === 'USER_SAMPLE' && !setup.sample)" @click="generateCards">生成 3 版风格卡</button>
        </div>
        <CommandFeedback :state="store.commandState('style-card-generate')" @confirm="store.confirmCommandResult('style-card-generate')" />
        <div v-if="setup.styleCard.candidates.length" class="setting-grid" style="margin-top:16px">
          <article v-for="candidate in setup.styleCard.candidates" :key="candidate.id" class="card pad">
            <h3 class="card-title">{{ styleSummary(candidate).title }}</h3>
            <ul><li v-for="line in styleSummary(candidate).lines" :key="line">{{ line }}</li></ul>
            <button class="btn small" @click="editCard(candidate, candidate.id)">选这一版并编辑</button>
          </article>
        </div>

        <div v-if="cardDraft" style="display:grid;gap:16px;margin-top:20px">
          <label class="field"><span class="field-label">名称</span><input v-model="cardDraft.label" class="input" /></label>
          <div class="setting-grid">
            <label class="field"><span class="field-label">叙述人称</span>
              <select v-model="cardDraft.narration.person" class="input"><option value="THIRD">第三人称</option><option value="FIRST">第一人称</option></select>
            </label>
            <label class="field"><span class="field-label">视角</span>
              <select v-model="cardDraft.narration.povPolicy" class="input"><option value="STRICT">只跟主角视角</option><option value="SHORT_CUTS_ALLOWED">允许短切到对手</option></select>
            </label>
            <label class="field"><span class="field-label">口吻</span><input v-model="cardDraft.narration.voice" class="input" /></label>
          </div>
          <div class="field"><span class="field-label">量化目标（最小 – 最大）</span>
            <div v-for="(range, key) in cardDraft.targets" :key="key" class="setting-grid">
              <span class="field-label">{{ TARGET_LABELS[key] ?? key }}</span>
              <input v-model.number="range.min" type="number" min="0" step="0.5" class="input" />
              <input v-model.number="range.max" type="number" min="0" step="0.5" class="input" />
            </div>
          </div>
          <label class="field"><span class="field-label">开篇结构（每行一步）</span><textarea v-model="cardDraft.openingStructure" class="textarea" /></label>
          <label class="field"><span class="field-label">写法规则（每行一条，4–10 条）</span><textarea v-model="cardDraft.rules" class="textarea" /></label>
          <label class="field"><span class="field-label">禁用写法（每行一条）</span><textarea v-model="cardDraft.forbidden" class="textarea" /></label>
          <CommandFeedback :state="store.commandState('style-card-confirm')" @confirm="store.confirmCommandResult('style-card-confirm')" />
          <div class="actions" style="justify-content:flex-end">
            <button class="btn" @click="cardEdit = null">取消</button>
            <button class="btn primary" :disabled="busy('style-card-confirm')" @click="confirmCard">确认这版风格卡</button>
          </div>
        </div>
      </section>
    </template>
  </div>
</template>
