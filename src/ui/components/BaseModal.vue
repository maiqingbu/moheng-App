<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'

const props = defineProps<{ open: boolean; title: string; subtitle?: string }>()
const emit = defineEmits<{ close: [] }>()
const modalWrap = ref<HTMLElement | null>(null)
const previousFocus = ref<HTMLElement | null>(null)

function focusableElements() {
  return Array.from(modalWrap.value?.querySelectorAll<HTMLElement>('button:not([disabled]), a[href], input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])') ?? [])
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    emit('close')
    return
  }
  if (event.key !== 'Tab') return
  const elements = focusableElements()
  if (!elements.length) return
  const first = elements[0]
  const last = elements[elements.length - 1]
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault()
    first.focus()
  }
}

watch(() => props.open, async open => {
  if (open) {
    previousFocus.value = document.activeElement instanceof HTMLElement ? document.activeElement : null
    await nextTick()
    focusableElements()[0]?.focus()
  } else {
    previousFocus.value?.focus()
    previousFocus.value = null
  }
})

onBeforeUnmount(() => previousFocus.value?.focus())
</script>
<template>
  <Teleport to="body">
    <div v-if="open" class="modal-backdrop" aria-hidden="true" @click="emit('close')" />
    <div v-if="open" ref="modalWrap" class="modal-wrap" role="dialog" aria-modal="true" :aria-label="title" @keydown="handleKeydown">
      <div class="modal">
        <div class="modal-head"><div><h2 style="margin:0;font-size:18px">{{ title }}</h2><div v-if="subtitle" class="muted" style="font-size:12px">{{ subtitle }}</div></div><button class="icon-btn" type="button" :aria-label="`关闭${title}`" @click="emit('close')">×</button></div>
        <div class="modal-body"><slot /></div>
        <div class="modal-foot"><slot name="footer" /></div>
      </div>
    </div>
  </Teleport>
</template>
