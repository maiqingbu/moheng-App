<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { CHAPTER_LENGTH_PRESETS, DEFAULT_CHAPTER_LENGTH_RANGE, chapterLengthLabel, isValidChapterLengthRange, type ChapterLengthRange } from '../presentation/chapterLength'

const props = defineProps<{
  modelValue: ChapterLengthRange | null | undefined
  /** Per-chapter use: offer "use the work's default" (emits null). */
  allowDefault?: boolean
  defaultRange?: ChapterLengthRange | null
  label?: string
}>()
const emit = defineEmits<{ 'update:modelValue': [value: ChapterLengthRange | null] }>()

const workDefault = computed(() => props.defaultRange ?? DEFAULT_CHAPTER_LENGTH_RANGE)
function choiceOf(value: ChapterLengthRange | null | undefined) {
  if (!value) return props.allowDefault ? 'default' : 'preset:0'
  const preset = CHAPTER_LENGTH_PRESETS.findIndex(item => item.min === value.min && item.max === value.max)
  return preset >= 0 ? `preset:${preset}` : 'custom'
}
const choice = ref(choiceOf(props.modelValue))
const custom = ref<ChapterLengthRange>({ ...(props.modelValue ?? workDefault.value) })
watch(() => props.modelValue, value => {
  // A user who chose "custom" stays there even when the range equals a preset.
  if (choice.value === 'custom') {
    if (value) custom.value = { ...value }
    return
  }
  choice.value = choiceOf(value)
})
const customValid = computed(() => isValidChapterLengthRange(custom.value))

function onChoice(next: string) {
  choice.value = next
  if (next === 'default') emit('update:modelValue', null)
  else if (next.startsWith('preset:')) emit('update:modelValue', { ...CHAPTER_LENGTH_PRESETS[Number(next.slice(7))] })
  else if (customValid.value) emit('update:modelValue', { ...custom.value })
}
function onCustom(bound: 'min' | 'max', raw: string) {
  custom.value = { ...custom.value, [bound]: Number(raw) }
  if (customValid.value) emit('update:modelValue', { ...custom.value })
}
</script>

<template>
  <div class="field" data-chapter-length-picker>
    <span class="field-label">{{ label ?? '每章字数区间' }}</span>
    <select class="select" :value="choice" @change="onChoice(($event.target as HTMLSelectElement).value)">
      <option v-if="allowDefault" value="default">{{ defaultRange ? `作品默认（${chapterLengthLabel(defaultRange)}）` : '作品默认' }}</option>
      <option v-for="(preset, index) in CHAPTER_LENGTH_PRESETS" :key="index" :value="`preset:${index}`">{{ chapterLengthLabel(preset) }}</option>
      <option value="custom">自定义</option>
    </select>
    <div v-if="choice === 'custom'" class="actions" style="margin-top:8px">
      <input class="input" type="number" min="500" max="10000" step="100" :value="custom.min" aria-label="最少字数" @input="onCustom('min', ($event.target as HTMLInputElement).value)" />
      <span class="muted">至</span>
      <input class="input" type="number" min="500" max="10000" step="100" :value="custom.max" aria-label="最多字数" @input="onCustom('max', ($event.target as HTMLInputElement).value)" />
    </div>
    <span v-if="choice === 'custom' && !customValid" class="help" role="alert">请填写整数区间：最少 500 字，最多 10000 字，且最少字数小于最多字数。</span>
    <span v-else class="help">规划会按这个区间安排场景篇幅；正文允许比上限多 10%，不在区间内会自动重写，最多两次。</span>
  </div>
</template>
