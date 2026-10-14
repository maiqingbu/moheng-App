<script setup lang="ts">
defineProps<{ locked: boolean }>()
</script>
<template>
  <div v-if="locked" class="banner info" role="status" aria-live="polite" data-write-lock>
    本作品正在生成，暂时只读。生成完成或暂停后即可继续修改；阅读和查看不受影响。
  </div>
</template>
