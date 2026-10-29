<template>
  <div class="amdl-lyric-group">
    <div ref="primary" class="amdl-primary-line">
      <KaraokeLine v-if="line.words?.length" :line="line" :position-ms="positionMs" />
      <div v-else class="amdl-lyric-line is-active">{{ line.text }}</div>
    </div>
    <div v-if="auxiliary" class="amdl-auxiliary-layout" :data-mode="auxiliaryMode" :style="{ '--amdl-auxiliary-font-size': `${auxiliarySize}px` }">
      <KaraokeLine v-if="pronunciationLine" :line="pronunciationLine" :position-ms="positionMs" />
      <div v-else class="amdl-lyric-line">{{ auxiliary.text }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { LyricLine, LyricsAuxiliaryMode } from '../types';
import { auxiliaryFontSize, localizedLine } from '../localization';
import KaraokeLine from './KaraokeLine.vue';

const props = defineProps<{ line: LyricLine; positionMs: number; auxiliaryMode: LyricsAuxiliaryMode }>();
const primary = ref<HTMLElement | null>(null);
const auxiliarySize = ref(24);
const auxiliary = computed(() => props.auxiliaryMode === 'translation'
  ? localizedLine(props.line.translations, true)
  : props.auxiliaryMode === 'pronunciation' ? localizedLine(props.line.pronunciations) : null);
const pronunciationLine = computed<LyricLine | null>(() => {
  const value = auxiliary.value;
  if (props.auxiliaryMode !== 'pronunciation' || !value?.words?.length ||
    !value.words.every(word => Number.isFinite(word.begin_ms) && Number.isFinite(word.end_ms) && word.end_ms > word.begin_ms)) return null;
  return { begin_ms: props.line.begin_ms, end_ms: props.line.end_ms, text: value.text, words: value.words };
});
let resizeObserver: ResizeObserver | null = null;
let themeObserver: MutationObserver | null = null;
let active = false;
function measureFont() {
  if (!active) return;
  const line = primary.value?.querySelector('.amdl-lyric-line');
  if (line) auxiliarySize.value = auxiliaryFontSize(parseFloat(getComputedStyle(line).fontSize));
}
watch(() => [props.line, props.auxiliaryMode], () => { void nextTick(measureFont); });
onMounted(() => {
  active = true;
  resizeObserver = new ResizeObserver(measureFont);
  if (primary.value) resizeObserver.observe(primary.value);
  const theme = document.querySelector('#amdl-runtime-theme');
  if (theme) { themeObserver = new MutationObserver(measureFont); themeObserver.observe(theme, { subtree: true, characterData: true, childList: true }); }
  measureFont();
});
onUnmounted(() => { active = false; resizeObserver?.disconnect(); themeObserver?.disconnect(); });
</script>

<style scoped>
.amdl-lyric-group { width: 100%; }
.amdl-auxiliary-layout { margin-top: 2px; text-align: center; font-size: var(--amdl-auxiliary-font-size); line-height: 1.25; }
.amdl-auxiliary-layout :deep(.amdl-lyric-line) { font-size: var(--amdl-auxiliary-font-size) !important; line-height: 1.25 !important; text-align: center !important; white-space: pre-wrap; overflow-wrap: anywhere; }
.amdl-lyric-group + .amdl-lyric-group { margin-top: 4px; }
</style>
