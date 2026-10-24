<script setup lang="ts">
import { computed } from 'vue'
import type { BlockReason, RunState } from '../../application/contracts'
const props = defineProps<{ state: RunState; reason: BlockReason }>()
const view = computed(() => {
  const reasonMap: Record<string,string> = {
    USER_PAUSE:'你主动暂停了制作。已正式提交的内容保持不变。', BUDGET_LIMIT:'预算已经达到当前上限，需要调整后继续。', DECISION_REQUIRED:'有一项关键选择需要你确认，系统不会在不确定方向上继续写。',
    POLICY_EXPIRED:'目标平台规则已经更新，需要重新确认适用版本。', SIGNING_REQUIRED:'前三章尚未通过开篇评估，第四章保持锁定。', SIGNING_INVALIDATED:'核心设定发生变化，原开篇评估回执已经失效。',
    CONTRACT_FAILURE:'当前内容与已确认创作规则冲突。', CANON_CONFLICT:'制作结果与正式作品事实发生冲突。', RETRY_EXHAUSTED:'自动修复次数已经用尽，需要你的决定。', PERSISTENCE_FAILURE:'本机数据提交失败，系统已停止继续生产以保护正式状态。',
    UNKNOWN_REMOTE_OUTCOME:'远端请求结果未知，等待处理决定。'
  }
  const map: Record<RunState,{title:string,tone:string,copy:string}> = {
    IDLE:{title:'等待开始',tone:'info',copy:'项目已准备好，尚未启动新的生产任务。'}, ACTIVE:{title:'制作正在进行',tone:'success',copy:'系统正在按已确认规则继续生产。'},
    PAUSED:{title:'制作已暂停',tone:'warning',copy:reasonMap[props.reason ?? ''] ?? '制作已暂停。'}, WAITING_DECISION:{title:'需要你的决定',tone:'warning',copy:reasonMap[props.reason ?? 'DECISION_REQUIRED']},
    BLOCKED:{title:'制作暂时停止',tone:'danger',copy:reasonMap[props.reason ?? ''] ?? '存在必须先解决的阻断原因。'}, FAILED:{title:'本次制作失败',tone:'danger',copy:'本次任务没有提交为正式产物，可以从最近一次正式进度恢复。'},
    COMPLETED:{title:'本阶段已完成',tone:'success',copy:'当前生产阶段已经完成并生成正式回执。'}
  }
  return map[props.state]
})
</script>
<template><div class="banner" :class="view.tone"><div><div class="banner-title">{{ view.title }}</div><div class="banner-copy">{{ view.copy }}</div></div><slot /></div></template>
