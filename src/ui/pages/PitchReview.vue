<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import PageHeader from '../components/PageHeader.vue'
import BaseModal from '../components/BaseModal.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'

const store = useProjectStore()
const router = useRouter()
const selected = ref<string[]>([])
const combineOpen = ref(false)
const constraintsOpen = ref(false)
const newConstraint = ref('保持“名字可被篡改”的核心机制，但减少过多规则解释；前三章必须让主角更主动。')
const commandKey = 'pitch-confirm'
const regenerateKey = 'pitch-regenerate'
const pitches = computed(()=>store.directions?.pitches ?? [])
onMounted(()=>void store.loadDirections())
const commandState = computed(()=>store.commandState(commandKey))
const regenerateState = computed(()=>store.commandState(regenerateKey))
const bookPlanState = computed(()=>store.commandState('book-plan-revise'))

function choose(id:string){ selected.value=[id] }
function toggleCombine(id:string){ selected.value = selected.value.includes(id) ? selected.value.filter(x=>x!==id) : [...selected.value,id].slice(0,2) }
async function regenerate(){ await store.runCommand(regenerateKey,{type:'pitch_regenerate',projectId:store.project?.id,payload:{adjustment:newConstraint.value,regeneratePitches:true}}); constraintsOpen.value=false }
async function reviseBookPlan(){ await store.runCommand('book-plan-revise',{type:'book_plan_revise',payload:{reason:'用户从故事方向页请求重新审视书级战略'}}) }
async function confirm(){
  await store.runCommand(commandKey,{type:'pitch_select',projectId:store.project?.id,payload:{pitchIds:selected.value,mode:selected.value.length>1?'combine':'single'}})
  setTimeout(()=>router.push(`/projects/${store.currentProjectId ?? ''}/setup`),350)
}
</script>

<template>
  <div class="page">
    <PageHeader title="规划 · 故事方向" subtitle="先明确作品方向与故事承诺，再进入故事核心和当前卷规划。">
      <button class="btn" @click="constraintsOpen=true">修改约束后重生成</button><button class="btn soft" @click="regenerate">重新推荐</button>
    </PageHeader>

    <div class="pitch-toolbar">
      <div><strong>当前选择：{{ selected.length ? pitches.filter(p=>selected.includes(p.id)).map(p=>p.name).join(' + ') : '尚未选择' }}</strong><div class="muted" style="font-size:11px">组合方案最多选择两份，系统会重新检查故事承诺、主角欲望、核心机制和主要冲突是否一致。</div></div>
      <div class="actions"><button class="btn" @click="combineOpen=true">组合方案</button><button class="btn primary" :disabled="!selected.length || ['submitting','accepted','processing'].includes(commandState.status)" @click="confirm">确认故事方向</button></div>
    </div>
    <CommandFeedback :state="commandState" /><CommandFeedback :state="regenerateState" />

    <section v-if="store.directions" class="card pad section" style="margin-top:14px">
      <div class="card-head"><div><span class="eyebrow">书级战略摘要</span><h2 class="card-title" style="margin-top:6px">全书战略 / 全书约束</h2><p class="card-subtitle">这里只展示会影响创作的核心内容，详细检查由系统自动完成。</p></div><button class="btn" @click="reviseBookPlan">修订书级战略</button></div>
      <CommandFeedback :state="bookPlanState" />
      <dl class="pitch-kv" style="margin-top:12px">
        <div><dt>全书承诺</dt><dd>{{ store.directions.strategy.storyPromise }}</dd></div>
        <div><dt>主角宏观方向</dt><dd>{{ store.directions.strategy.protagonistDirection }}</dd></div>
        <div><dt>终局边界</dt><dd>{{ store.directions.strategy.endingBoundary }}</dd></div>
        <div><dt>篇幅预算</dt><dd>{{ store.directions.strategy.lengthBudget }}</dd></div>
        <div><dt>当前全书战略</dt><dd>{{ store.directions.strategy.currentBookPlanVersion }}</dd></div>
        <div><dt>当前全书约束</dt><dd>{{ store.directions.strategy.currentMainContractVersion }}</dd></div>
      </dl>
    </section>

    <section class="pitch-grid section" style="margin-top:14px">
      <article v-for="pitch in pitches" :key="pitch.id" class="pitch-card" :class="{selected:selected.includes(pitch.id),recommended:pitch.recommended}">
        <div class="pitch-card-head">
          <span class="badge">{{ pitch.variant }}</span>
          <h2 style="margin:10px 0 3px;font-family:var(--font-serif);font-size:22px">{{ pitch.name }}</h2>
          <div class="muted" style="font-size:12px">{{ pitch.platform }} · {{ pitch.genre }}</div>
          <p style="margin:13px 0 0;font-size:13px;line-height:1.7">{{ pitch.promise }}</p>
        </div>
        <div class="pitch-card-body">
          <dl class="pitch-kv">
            <div><dt>目标读者</dt><dd>{{ pitch.audience }}</dd></div>
            <div><dt>核心情绪价值</dt><dd>{{ pitch.emotion }}</dd></div>
            <div><dt>核心回报</dt><dd>{{ pitch.reward }}</dd></div>
            <div><dt>主角欲望</dt><dd>{{ pitch.protagonistDesire }}</dd></div>
            <div><dt>独特机制</dt><dd>{{ pitch.mechanism }}</dd></div>
            <div><dt>世界冲突</dt><dd>{{ pitch.worldConflict }}</dd></div>
            <div><dt>长篇升级空间</dt><dd>{{ pitch.longArc }}</dd></div>
            <div><dt>前三章钩子</dt><dd>{{ pitch.openingHook }}</dd></div>
            <div><dt>第一卷目标</dt><dd>{{ pitch.volumeGoal }}</dd></div>
            <div><dt>预期篇幅</dt><dd>{{ pitch.expectedLength }}</dd></div>
            <div><dt>商业优势</dt><dd>{{ pitch.commercialEdge }}</dd></div>
            <div><dt>同质化风险</dt><dd>{{ pitch.homogeneityRisk }}</dd></div>
            <div><dt>原创性说明</dt><dd>{{ pitch.originality }}</dd></div>
            <div><dt>创作难点</dt><dd>{{ pitch.difficulty }}</dd></div>
            <div><dt>失败风险</dt><dd>{{ pitch.failureRisk }}</dd></div>
            <div style="padding:10px;border-radius:9px;background:var(--pine-50)"><dt style="color:var(--pine-700)">系统推荐理由</dt><dd>{{ pitch.aiReason }}</dd></div>
          </dl>
          <button class="btn large" :class="selected.includes(pitch.id)?'primary':''" style="width:100%;margin-top:16px" @click="choose(pitch.id)">{{ selected.includes(pitch.id)?'已选择':'选择这个方向' }}</button>
        </div>
      </article>
    </section>

    <BaseModal :open="combineOpen" title="组合两个故事方向" subtitle="组合不会简单拼接文本，而是重新编译一个一致的新立项。" @close="combineOpen=false">
      <div style="display:grid;gap:10px"><button v-for="pitch in pitches" :key="pitch.id" class="card pad" style="text-align:left" :style="selected.includes(pitch.id)?'border-color:var(--pine-400)':''" @click="toggleCombine(pitch.id)"><div style="display:flex;justify-content:space-between;gap:10px"><strong>{{ pitch.name }}</strong><span class="badge" :class="selected.includes(pitch.id)?'pine':''">{{ selected.includes(pitch.id)?'已选':'未选' }}</span></div><div class="muted" style="font-size:12px;margin-top:4px">{{ pitch.promise }}</div></button></div>
      <div class="banner info" style="margin-top:14px"><div><div class="banner-title">组合后会重新检查实质差异与内部一致性</div><div class="banner-copy">故事承诺、主角欲望、机制和世界冲突不允许互相矛盾。</div></div></div>
      <template #footer><button class="btn" @click="combineOpen=false">取消</button><button class="btn primary" :disabled="selected.length<2" @click="combineOpen=false">使用这两个方向组合</button></template>
    </BaseModal>

    <BaseModal :open="constraintsOpen" title="修改约束后重新生成" subtitle="只改你关心的部分，其他已经确认的意图保持不变。" @close="constraintsOpen=false">
      <label class="field"><span class="field-label">补充或修改约束</span><textarea v-model="newConstraint" class="textarea" /></label>
      <CommandFeedback :state="regenerateState" /><template #footer><button class="btn" @click="constraintsOpen=false">取消</button><button class="btn primary" :disabled="['submitting','accepted','processing'].includes(regenerateState.status)" @click="regenerate">重新生成三份方向</button></template>
    </BaseModal>
  </div>
</template>
