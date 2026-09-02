<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import PageHeader from '../components/PageHeader.vue'
import BaseDrawer from '../components/BaseDrawer.vue'
import CommandFeedback from '../components/CommandFeedback.vue'
import { useProjectStore } from '../stores/projectStore'
import { statusLabel } from '../presentation/productLanguage'

const store=useProjectStore()
const tab=ref<'relations'|'timeline'|'foreshadow'|'knowledge'>('relations')
const selectedCharacter=ref<string|null>('shen')
const revisionOpen=ref(false)
const revisionText=ref('')
const revisionKey='canon-revision'
const canon=computed(()=>store.canon)
onMounted(()=>void store.loadCanon())
const selected=computed(()=>canon.value?.characters.items.find(c=>c.id===selectedCharacter.value) ?? null)
const relatedList=computed(()=>{
  const sel=selected.value
  if(!sel||!canon.value) return []
  const chars=canon.value.characters.items
  return canon.value.relations.items
    .filter(r=>r.from===sel.id||r.to===sel.id)
    .map(r=>{
      const otherId=r.from===sel.id?r.to:r.from
      const other=chars.find(c=>c.id===otherId)
      return { key:`${r.from}-${r.to}`, name:other?.name ?? '未知', label:r.label }
    })
})
const nodePositions:Record<string,{x:number,y:number}>={shen:{x:44,y:50},xuan:{x:23,y:24},liu:{x:20,y:72},mask:{x:76,y:30}}
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

    <section v-if="tab==='relations'" class="production-layout">
      <div class="graph-stage">
        <svg viewBox="0 0 1000 560" preserveAspectRatio="none">
          <line x1="440" y1="280" x2="230" y2="135" stroke="var(--line-strong)" stroke-width="1.5" />
          <line x1="440" y1="280" x2="200" y2="404" stroke="var(--line-strong)" stroke-width="1.5" />
          <line x1="230" y1="135" x2="760" y2="168" stroke="var(--red)" stroke-width="1.5" stroke-dasharray="6 6" />
        </svg>
        <button v-for="c in canon.characters.items" :key="c.id" class="graph-node" :class="{primary:c.id==='shen',candidate:c.status==='候选影响'}" :style="{left:`${nodePositions[c.id]?.x ?? 50}%`,top:`${nodePositions[c.id]?.y ?? 50}%`}" @click="selectedCharacter=c.id">
          <strong>{{ c.name }}</strong><span>{{ c.role }} · {{ statusLabel(c.status) }}</span>
        </button>
      </div>
      <aside class="card pad">
        <template v-if="selected"><div style="display:flex;justify-content:space-between;gap:8px"><span class="badge" :class="selected.status==='正式'?'success':'warning'">{{ statusLabel(selected.status) }}</span><span class="muted" style="font-size:10px">{{ selected.location }}</span></div><h2 style="margin:10px 0 4px;font-family:var(--font-serif)">{{ selected.name }}</h2><p class="muted">{{ selected.note }}</p><div class="divider" /><h3 style="font-size:13px">当前关系</h3><div style="display:grid;gap:8px"><div v-for="r in relatedList" :key="r.key" class="card pad" style="padding:10px"><strong style="font-size:12px">{{ r.name }}</strong><div class="muted" style="font-size:11px">{{ r.label }}</div></div></div></template>
      </aside>
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
