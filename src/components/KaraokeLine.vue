<template>
  <div class="amdl-karaoke-row">
    <svg class="amdl-karaoke-definitions" aria-hidden="true" width="0" height="0">
      <defs>
        <clipPath :id="`${clipId}-normal`" clipPathUnits="userSpaceOnUse"><path :d="masks.normal" clip-rule="evenodd" /></clipPath>
        <clipPath :id="`${clipId}-active`" clipPathUnits="userSpaceOnUse"><path :d="masks.active" /></clipPath>
      </defs>
    </svg>
    <span class="amdl-karaoke-text">
      <span ref="normalText" class="amdl-lyric-line amdl-karaoke-glyphs" :style="{ clipPath: geometry.width > 0 ? `url(#${clipId}-normal)` : undefined }">{{ text }}</span>
      <span ref="activeText" aria-hidden="true" class="amdl-lyric-line is-active amdl-karaoke-glyphs amdl-karaoke-active" :style="{ clipPath: `url(#${clipId}-active)`, backgroundSize: `${Math.max(1, masks.extent)}px 100%` }">{{ text }}</span>
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, shallowRef, useId, watch } from 'vue';
import type { LyricLine } from '../types';
import { karaokeMasks, type GlyphRect, type KaraokeGeometry } from '../karaoke';

const props = defineProps<{ line: LyricLine; positionMs: number }>();
const clipId = `amdl-karaoke-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`;
const normalText = shallowRef<HTMLElement | null>(null);
const activeText = shallowRef<HTMLElement | null>(null);
const text = computed(() => props.line.words.map((word) => word.text).join('') || props.line.text);
const geometry = shallowRef<KaraokeGeometry>({ width: 0, height: 0, padding: 32, words: [] });
const masks = computed(() => karaokeMasks(props.line.words, geometry.value, props.positionMs));
let resizeObserver: ResizeObserver | null = null;
let themeObserver: MutationObserver | null = null;
let measurementFrame: number | null = null;
let mounted = false;

function measure() {
  measurementFrame = null;
  const element = normalText.value;
  const node = element?.firstChild;
  if (!element || !node || node.nodeType !== Node.TEXT_NODE) return;
  const box = element.getBoundingClientRect();
  const style = getComputedStyle(element);
  const width = parseFloat(style.width) || element.clientWidth;
  const height = parseFloat(style.height) || element.clientHeight;
  if (width <= 0 || height <= 0 || box.width <= 0 || box.height <= 0) return;
  // 暂停时父容器会缩放到 95%，DOMRange 的屏幕坐标必须换回字形的本地坐标。
  const scaleX = box.width / width, scaleY = box.height / height;
  const lineHeight = parseFloat(style.lineHeight) || height;
  const effectSizes = [style, activeText.value ? getComputedStyle(activeText.value) : style].flatMap((paint) => [...`${paint.textShadow} ${paint.webkitTextStrokeWidth}`.matchAll(/-?[\d.]+px/g)].map((match) => Math.abs(parseFloat(match[0]))));
  const padding = Math.max(32, ...effectSizes.map((size) => size * 2 + 2));
  const words: GlyphRect[][] = [];
  let offset = 0;
  const range = document.createRange();
  for (const word of props.line.words) {
    const end = Math.min(text.value.length, offset + word.text.length);
    range.setStart(node, offset); range.setEnd(node, end);
    const regions = Array.from(range.getClientRects()).filter((rect) => rect.width > 0).map((rect) => {
      const row = Math.max(0, Math.floor(((rect.top - box.top) / scaleY + rect.height / scaleY / 2) / lineHeight));
      const top = row * lineHeight;
      const bottom = Math.min(height, top + lineHeight);
      const before = row === 0 ? padding : 0;
      const after = bottom >= height - 0.5 ? padding : 0;
      return { x: (rect.left - box.left) / scaleX, y: top - before, width: rect.width / scaleX, height: bottom - top + before + after };
    });
    words.push(regions); offset = end;
  }
  const rowEnds = new Map<number, number>();
  for (const rect of words.flat()) rowEnds.set(rect.y, Math.max(rowEnds.get(rect.y) ?? 0, rect.x + rect.width));
  for (const rect of words.flat()) rect.endsRow = rect.x + rect.width >= (rowEnds.get(rect.y) ?? width) - 0.01;
  geometry.value = { width, height, padding, words };
}
function scheduleMeasure() {
  if (mounted && measurementFrame === null) measurementFrame = requestAnimationFrame(measure);
}
watch(() => props.line, scheduleMeasure, { flush: 'post' });
onMounted(() => {
  mounted = true;
  resizeObserver = new ResizeObserver(scheduleMeasure);
  if (normalText.value) resizeObserver.observe(normalText.value);
  const theme = document.querySelector('#amdl-runtime-theme');
  if (theme) { themeObserver = new MutationObserver(scheduleMeasure); themeObserver.observe(theme, { childList: true, characterData: true, subtree: true }); }
  document.fonts?.addEventListener('loadingdone', scheduleMeasure);
  void document.fonts?.ready.then(scheduleMeasure);
  scheduleMeasure();
});
onUnmounted(() => {
  mounted = false;
  resizeObserver?.disconnect(); themeObserver?.disconnect();
  document.fonts?.removeEventListener('loadingdone', scheduleMeasure);
  if (measurementFrame !== null) cancelAnimationFrame(measurementFrame);
});
</script>

<style scoped>
.amdl-karaoke-definitions { position: absolute; pointer-events: none; }
.amdl-karaoke-text { display: inline-block; position: relative; max-width: 100%; vertical-align: bottom; }
.amdl-karaoke-glyphs { display: block; white-space: pre-wrap; background-repeat: no-repeat; background-position: 0 0; }
.amdl-karaoke-active { position: absolute; inset: 0; pointer-events: none; }
</style>
