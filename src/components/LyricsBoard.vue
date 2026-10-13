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
        <div v-for="line in displayLines" :key="`${line.begin_ms}-${line.text}`" class="amdl-lyric-line" :class="{ 'is-active': !line.words?.length && isLineActive(line) }">
          <template v-if="line.words?.length">
            <span v-for="(word, wordIndex) in line.words" :key="`${word.begin_ms}-${wordIndex}`" class="relative inline-block">
              <span class="amdl-lyric-line inline-block whitespace-pre"
                    :style="{ clipPath: `inset(0 0 0 ${wordProgress(word) * 100}%)` }">{{ word.text }}</span>
              <span class="amdl-lyric-line is-active absolute inset-0 whitespace-pre"
                    :style="{ clipPath: `inset(0 calc(100% - ${wordProgress(word) * 100}%) 0 0)` }">{{ word.text }}</span>
            </span>
          </template>
          <template v-else>{{ line.text }}</template>
        </div>
      </template>
    </div>

  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { LyricWord, LyricsDocument, LyricsLoadState } from '../types';

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

/** 计算当前应该高亮显示的歌词 */
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
  if (activeLineIndex.value === props.lyrics.lines.length - 1) return [];
  return [props.lyrics.lines[activeLineIndex.value]];
});

const displayMode = computed<'status' | 'intro' | 'interlude' | 'outro' | 'line'>(() => {
  if (props.lyricsState !== 'loaded' || !props.lyrics?.lines.length) return 'status';
  if (activeLineIndex.value < 0) return 'intro';
  if (currentLines.value.length) return 'line';
  const entry = timeline.value[activeLineIndex.value];
  const next = props.lyrics.lines[activeLineIndex.value + 1];
  if (!next) return 'outro';
  if (next.begin_ms - entry.end >= INTERLUDE_GAP_MS) return 'interlude';
  return 'line';
});

const displayText = computed(() => {
  if (props.title === '等待播放...' || (!props.title.trim() && !props.artist.trim())) return '等待播放...';
  if (displayMode.value === 'intro' || displayMode.value === 'outro') return [props.title, props.artist].filter(Boolean).join(' - ');
  if (displayMode.value === 'interlude') return '● ● ●';
  if (props.lyricsState === 'loading') return '正在搜索歌词...';
  if (props.lyricsState === 'notFound') return '暂无匹配歌词';
  if (props.lyricsState === 'error') return props.lyricsMessage || '歌词加载失败';
  return '';
});

function isLineActive(line: { begin_ms: number; end_ms: number | null }) {
  return currentLines.value.some((active) => active === line);
}

function wordProgress(word: LyricWord) {
  const duration = Math.max(1, word.end_ms - word.begin_ms);
  return Math.max(0, Math.min(1, (props.positionMs - word.begin_ms) / duration));
}

</script>
