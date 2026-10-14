<template>
  <div class="amdl-lyrics w-full h-full flex flex-col justify-center items-center bg-transparent select-none px-6 pointer-events-none">
    <!-- 歌词展示 -->
    <div class="amdl-lyrics-stack transition-all duration-300"
         :class="isPlaying ? 'opacity-100 scale-100' : 'opacity-50 scale-95'"
    >
      <template v-if="displayMode !== 'line'">
        <div class="amdl-lyric-line is-active">{{ displayText }}</div>
      </template>
      <template v-else-if="displayLines.length">
        <template v-for="line in displayLines" :key="`${line.begin_ms}-${line.text}`">
          <KaraokeLine v-if="line.words?.length" :line="line" :position-ms="positionMs" />
          <div v-else class="amdl-lyric-line is-active">{{ line.text }}</div>
        </template>
      </template>
    </div>

  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { LyricsDocument, LyricsLoadState } from '../types';
import KaraokeLine from './KaraokeLine.vue';

const props = defineProps<{
  title: string;
  artist: string;
  isPlaying: boolean;
  positionMs: number;
  durationMs?: number;
  lyrics: LyricsDocument | null;
  lyricsState: LyricsLoadState;
  lyricsMessage: string | null;
}>();

/** 计算当前显示的歌词；逐行歌词在整个显示期间保持高亮。 */
const INTERLUDE_GAP_MS = 8_000;

const timeline = computed(() => {
  const lines = props.lyrics?.lines ?? [];
  const trackEnd = props.durationMs && props.durationMs > 0 ? props.durationMs : props.lyrics?.duration_ms;
  const nextBegins: (number | undefined)[] = [];
  let nextBegin: number | undefined;
  for (let index = lines.length - 1; index >= 0; index--) {
    if (index + 1 < lines.length && lines[index + 1].begin_ms > lines[index].begin_ms) nextBegin = lines[index + 1].begin_ms;
    nextBegins[index] = nextBegin;
  }
  let prefixEnd = 0;
  const entries = lines.map((line, index) => {
    const wordEnd = line.words.length ? Math.max(...line.words.map((word) => word.end_ms)) : null;
    const end = line.end_ms ?? wordEnd ?? nextBegins[index] ?? trackEnd ?? Infinity;
    prefixEnd = Math.max(prefixEnd, end);
    return { line, end, prefixEnd };
  });
  return entries;
});

const activeLineIndex = computed(() => {
  const lines = props.lyrics?.lines ?? [];
  let left = 0, right = lines.length;
  while (left < right) {
    const middle = Math.floor((left + right) / 2);
    if (lines[middle].begin_ms <= props.positionMs) left = middle + 1;
    else right = middle;
  }
  return left - 1;
});

const currentLines = computed(() => {
  const active = [];
  for (let index = activeLineIndex.value; index >= 0 && timeline.value[index].prefixEnd > props.positionMs; index -= 1) {
    const entry = timeline.value[index];
    if (entry.end > props.positionMs) active.unshift(entry.line);
    if (active.length === 3) break;
  }
  return active;
});

const displayLines = computed(() => {
  if (currentLines.value.length) return currentLines.value;
  if (activeLineIndex.value < 0 || !props.lyrics) return [];
  // 短句间隙及尾奏保留上一句，逐字歌词的进度会自然停在全部高亮。
  return [props.lyrics.lines[activeLineIndex.value]];
});

const displayMode = computed<'status' | 'intro' | 'interlude' | 'line'>(() => {
  if (props.lyricsState !== 'loaded' || !props.lyrics?.lines.length) return 'status';
  if (activeLineIndex.value < 0) return 'intro';
  if (currentLines.value.length) return 'line';
  const entry = timeline.value[activeLineIndex.value];
  const next = props.lyrics.lines[activeLineIndex.value + 1];
  if (next && next.begin_ms - entry.end >= INTERLUDE_GAP_MS) return 'interlude';
  return 'line';
});

const displayText = computed(() => {
  if (props.title === '等待播放...' || (!props.title.trim() && !props.artist.trim())) return '等待播放...';
  if (displayMode.value === 'intro') return [props.title, props.artist].filter(Boolean).join(' - ');
  if (displayMode.value === 'interlude') return '● ● ●';
  if (props.lyricsState === 'loading') return '正在搜索歌词...';
  if (props.lyricsState === 'notFound') return '暂无匹配歌词';
  if (props.lyricsState === 'error') return props.lyricsMessage || '歌词加载失败';
  return '';
});

</script>
