<script setup lang="ts">
import { resumeConditionKinds, type ResumeConditionDraft } from '../flows/resumeCondition'

const props = defineProps<{ modelValue: ResumeConditionDraft }>()
const emit = defineEmits<{ 'update:modelValue': [value: ResumeConditionDraft] }>()

function update(fields: Partial<ResumeConditionDraft>) {
  emit('update:modelValue', { ...props.modelValue, ...fields })
}
</script>

<template>
  <div class="resume-condition-fields" style="display:grid;gap:8px">
    <label style="display:grid;gap:4px">
      <span>兑现条件类型</span>
      <select :value="modelValue.kind" @change="update({ kind: ($event.target as HTMLSelectElement).value as ResumeConditionDraft['kind'] })">
        <option v-for="item in resumeConditionKinds" :key="item.value" :value="item.value">{{ item.label }}</option>
      </select>
    </label>
    <template v-if="modelValue.kind === 'NEXT_MEETING'">
      <label style="display:grid;gap:4px">人物 A<input :value="modelValue.entityA" placeholder="人物 A 的实体引用" @input="update({ entityA: ($event.target as HTMLInputElement).value })" /></label>
      <label style="display:grid;gap:4px">人物 B<input :value="modelValue.entityB" placeholder="人物 B 的实体引用" @input="update({ entityB: ($event.target as HTMLInputElement).value })" /></label>
    </template>
    <label v-else style="display:grid;gap:4px">
      <span>{{ modelValue.kind === 'ENTITY_PRESENT' ? '人物或实体引用' : modelValue.kind === 'KNOWLEDGE_ACQUIRED' ? '知识引用' : modelValue.kind === 'TASK_SATISFIED' ? '任务引用' : modelValue.kind === 'AFTER_CHAPTER' ? '章节序号' : '卷引用' }}</span>
      <input
        :value="modelValue.value"
        :type="modelValue.kind === 'AFTER_CHAPTER' ? 'number' : 'text'"
        :min="modelValue.kind === 'AFTER_CHAPTER' ? 0 : undefined"
        :placeholder="modelValue.kind === 'AFTER_CHAPTER' ? '输入非负章节序号' : '填写对应的正式引用'"
        @input="update({ value: ($event.target as HTMLInputElement).value })"
      />
    </label>
  </div>
</template>
