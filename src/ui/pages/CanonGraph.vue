<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import BaseDrawer from '../components/BaseDrawer.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { layoutCanonGraphNodes } from '../presentation/canonGraphLayout'
import { statusLabel } from '../presentation/productLanguage'

const store=useProjectStore()
const tab=ref<'relations'|'timeline'|'foreshadow'|'knowledge'>('relations')
const selectedCharacter=ref<string|null>(null)
const revisionOpen=ref(false)
const revisionText=ref('')
const revisionKey='canon-revision'
const canon=computed(()=>store.canon)
const characters=computed(()=>canon.value?.characters.items ?? [])
const nodePositions=computed(()=>layoutCanonGraphNodes(characters.value.map(character=>character.id)))
onMounted(()=>void store.loadCanon())
const selected=computed(()=>characters.value.find(c=>c.id===selectedCharacter.value) ?? characters.value[0] ?? null)
const graphRelations=computed(()=>{
  const positions=nodePositions.value
  return (canon.value?.relations.items ?? []).flatMap((relation,index)=>{
    const from=positions.get(relation.from)
    const to=positions.get(relation.to)
    if(!from||!to) return []
    return [{
      ...relation,
      key:`${relation.from}-${relation.to}-${index}`,
      x1:from.x*10,
      y1:from.y*5.6,
      x2:to.x*10,
      y2:to.y*5.6,
    }]
  })
})
const relatedList=computed(()=>{
  const sel=selected.value
  if(!sel||!canon.value) return []
  const characterIds=new Set(characters.value.map(character=>character.id))
  return canon.value.relations.items
    .filter(r=>(r.from===sel.id||r.to===sel.id)&&characterIds.has(r.from)&&characterIds.has(r.to))
    .map(r=>{
      const otherId=r.from===sel.id?r.to:r.from
      const other=characters.value.find(c=>c.id===otherId)!
      return { key:`${r.from}-${r.to}`, name:other.name, label:r.label }
    })
})
const revisionState=computed(()=>store.commandState(revisionKey))
const toneLabel={knows:'已知',believes:'相信/误解',conceals:'刻意隐瞒',unknown:'未知'} as Record<string,string>
async function submitRevision(){ await store.runCommand(revisionKey,{type:'change_request_create',projectId:store.project?.id,payload:{kind:'canon_revision',request:revisionText.value}}); revisionOpen.value=false }
</script>

<template>
  <div class="page" v-if="canon">
    <PageHeader title="作品图谱" subtitle="这里展示正式作品事实、人物关系、伏笔和角色知识差。制作稿影响会单独标记，绝不会污染正式图谱。">
      <button class="btn" @click="revisionOpen=true">发起设定修订</button>
    </PageHeader>

    <div class="graph-tabs">
      <button class="btn small" :class="tab==='relations'?'primary':''" @click="tab='relations'">人物关系</button>
      <button class="btn small" :class="tab==='timeline'?'primary':''" @click="tab='timeline'">时间线</button>
      <button class="btn small" :class="tab==='foreshadow'?'primary':''" @click="tab='foreshadow'">伏笔与谜团</button>
      <button class="btn small" :class="tab==='knowledge'?'primary':''" @click="tab='knowledge'">角色知识差</button>
    </div>

    <section v-if="tab==='relations'" class="section">
      <div v-if="characters.length" class="production-layout">
        <div class="graph-stage">
          <svg viewBox="0 0 1000 560" preserveAspectRatio="none">
            <line v-for="relation in graphRelations" :key="relation.key" :x1="relation.x1" :y1="relation.y1" :x2="relation.x2" :y2="relation.y2" :stroke="relation.tone==='negative'?'var(--red)':'var(--line-strong)'" stroke-width="1.5" :stroke-dasharray="relation.tone==='negative'?'6 6':undefined" />
          </svg>
          <button v-for="c in characters" :key="c.id" class="graph-node" :class="{primary:c.role==='主角',candidate:c.status==='候选影响'}" :style="{left:`${nodePositions.get(c.id)?.x}%`,top:`${nodePositions.get(c.id)?.y}%`}" @click="selectedCharacter=c.id">
            <strong>{{ c.name }}</strong><span>{{ c.role }} · {{ statusLabel(c.status) }}</span>
          </button>
        </div>
        <aside class="card pad">
          <template v-if="selected"><div style="display:flex;justify-content:space-between;gap:8px"><span class="badge" :class="selected.status==='正式'?'success':'warning'">{{ statusLabel(selected.status) }}</span><span class="muted" style="font-size:10px">{{ selected.location }}</span></div><h2 style="margin:10px 0 4px;font-family:var(--font-serif)">{{ selected.name }}</h2><p class="muted">{{ selected.note }}</p><div class="divider" /><h3 style="font-size:13px">当前关系</h3><div style="display:grid;gap:8px"><div v-for="r in relatedList" :key="r.key" class="card pad" style="padding:10px"><strong style="font-size:12px">{{ r.name }}</strong><div class="muted" style="font-size:11px">{{ r.label }}</div></div></div></template>
        </aside>
      </div>
      <div v-else class="empty card"><strong>暂无正式人物</strong></div>
    </section>

    <section v-if="tab==='timeline'" class="card pad">
      <div class="card-head"><div><h2 class="card-title">故事时间线</h2><p class="card-subtitle">候选章节只能作为影响预览，不会与正式事实混在一起。</p></div></div>
      <div class="timeline"><div v-for="(item,index) in canon.timeline.items" :key="item.chapter" class="timeline-item" :class="item.status==='official'?'done':'locked'"><div class="timeline-marker">{{ item.status==='official'?'✓':index+1 }}</div><div><div style="display:flex;justify-content:space-between;gap:10px"><strong>{{ item.chapter }} · {{ item.title }}</strong><span class="badge" :class="item.status==='official'?'success':'warning'">{{ item.status==='official'?'正式事实':'制作影响预览' }}</span></div><div class="muted" style="font-size:12px">{{ item.detail }}</div></div></div></div>
    </section>

    <section v-if="tab==='foreshadow'" class="ledger-list">
      <article v-for="f in canon.foreshadows.items" :key="f.id" class="ledger-row"><div style="display:flex;align-items:flex-start;justify-content:space-between;gap:12px"><div><span class="badge" :class="f.status==='resolved'?'success':'warning'">{{ f.status==='resolved'?'已回收':'待回收' }}</span><h3 style="margin:8px 0 3px">{{ f.title }}</h3><p class="muted" style="font-size:12px">{{ f.detail }}</p></div><div style="text-align:right;font-size:11px"><div>埋设：<strong>{{ f.planted }}</strong></div><div class="muted">计划：{{ f.planned }}</div></div></div></article>
    </section>

    <section v-if="tab==='knowledge'" class="knowledge-grid">
      <article v-for="k in canon.knowledge.items" :key="`${k.character}-${k.topic}`" class="knowledge-card"><span class="badge" :class="k.state==='conceals'?'danger':k.state==='knows'?'success':k.state==='believes'?'warning':'info'">{{ toneLabel[k.state] }}</span><h3 style="margin:9px 0 3px">{{ k.character }}</h3><div style="font-weight:700;font-size:12px">{{ k.topic }}</div><p class="muted" style="font-size:12px;margin-top:4px">{{ k.detail }}</p></article>
    </section>

    <BaseDrawer :open="revisionOpen" title="发起设定修订" subtitle="核心规则修改会先计算影响范围，必要时使旧开篇评估回执失效。" @close="revisionOpen=false">
      <div class="banner warning"><div><div class="banner-title">不会直接改写正式图谱</div><div class="banner-copy">你的修改先成为修订提案；系统计算受影响章节、伏笔、关系和签约状态后再提交。</div></div></div>
      <label class="field section"><span class="field-label">你想改什么？</span><textarea v-model="revisionText" class="textarea" placeholder="例如：我想让玄机子其实并不知道沈无悔的真实姓名，只知道断剑来自旧案。" /></label>
      <CommandFeedback :state="revisionState" /><div class="actions"><button class="btn" @click="revisionOpen=false">取消</button><button class="btn primary" :disabled="!revisionText.trim() || ['submitting','accepted','processing'].includes(revisionState.status)" @click="submitRevision">分析影响范围</button></div>
    </BaseDrawer>
  </div>
</template>
