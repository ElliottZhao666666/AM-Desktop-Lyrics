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
        <div v-for="line in displayLines" :key="`${line.begin_ms}-${line.text}`" class="amdl-lyric-line" :class="{ 'is-active': isLineActive(line), 'has-word-timeline': Boolean(line.words?.length) }">
          <template v-if="line.words?.length">
            <span v-for="(word, wordIndex) in line.words" :key="`${word.begin_ms}-${wordIndex}`" class="amdl-lyric-word" :style="wordStyle(word)">
              <span v-if="wordProgress(word) > 0" class="amdl-lyric-word-highlight" :class="{ 'is-active': true }">{{ word.text }}</span>{{ word.text }}
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
  lyrics: LyricsDocument | null;
  lyricsState: LyricsLoadState;
  lyricsMessage: string | null;
}>();

/** 计算当前应该高亮显示的歌词 */
const INTERLUDE_GAP_MS = 8_000;

const activeLineIndex = computed(() => {
  if (!props.lyrics?.lines.length || props.positionMs < props.lyrics.lines[0].begin_ms) return -1;
  let index = 0;
  for (let i = 1; i < props.lyrics.lines.length; i += 1) {
    if (props.lyrics.lines[i].begin_ms > props.positionMs) break;
    index = i;
  }
  return index;
});

const currentLines = computed(() => {
  if (!props.lyrics) return [];
  return props.lyrics.lines
    .filter((line) => line.begin_ms <= props.positionMs && (line.end_ms === null || line.end_ms > props.positionMs))
    .slice(-3);
});

const displayLines = computed(() => {
  if (currentLines.value.length) return currentLines.value;
  if (activeLineIndex.value < 0 || !props.lyrics) return [];
  return [props.lyrics.lines[activeLineIndex.value]];
});

const displayMode = computed<'status' | 'intro' | 'interlude' | 'line'>(() => {
  if (props.lyricsState !== 'loaded' || !props.lyrics?.lines.length) return 'status';
  if (activeLineIndex.value < 0) return 'intro';
  const line = currentLines.value[0] ?? props.lyrics.lines[activeLineIndex.value];
  const next = props.lyrics.lines[activeLineIndex.value + 1];
  if (!next) return 'line';
  const lineEnd = line.end_ms ?? line.begin_ms;
  if (next.begin_ms - lineEnd >= INTERLUDE_GAP_MS && props.positionMs >= lineEnd) return 'interlude';
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

function isLineActive(line: { begin_ms: number; end_ms: number | null }) {
  return props.lyrics?.lines[activeLineIndex.value] === line;
}

function wordProgress(word: LyricWord) {
  const duration = Math.max(1, word.end_ms - word.begin_ms);
  return Math.max(0, Math.min(1, (props.positionMs - word.begin_ms) / duration));
}

function wordStyle(word: LyricWord) {
  return {
    '--word-progress': `${wordProgress(word) * 100}%`,
  } as Record<string, string>;
}

</script>

<style scoped>
.amdl-lyric-word {
  position: relative;
  display: inline-block;
}

.amdl-lyric-word-highlight {
  position: absolute;
  inset: 0;
  overflow: hidden;
  white-space: pre;
  clip-path: inset(0 calc(100% - var(--word-progress)) 0 0);
  pointer-events: none;
}
</style>
