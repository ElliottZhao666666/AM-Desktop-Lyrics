<template>
  <div class="w-full h-full flex flex-col justify-center items-center bg-transparent text-white select-none px-6 pointer-events-none">
    <!-- 歌词展示 -->
    <div class="text-3xl font-black bg-clip-text text-transparent bg-gradient-to-r from-pink-400 via-fuchsia-400 to-purple-500 drop-shadow-lg transition-all duration-300"
         :class="isPlaying ? 'opacity-100 scale-100' : 'opacity-50 scale-95'"
    >
      <template v-if="displayMode !== 'line'">
        {{ displayText }}
      </template>
      <template v-else-if="currentLine">
        {{ currentLine.text }}
      </template>
    </div>

  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { LyricsDocument, LyricsLoadState } from '../types';

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

const currentLine = computed(() => activeLineIndex.value >= 0 ? props.lyrics?.lines[activeLineIndex.value] ?? null : null);

const displayMode = computed<'status' | 'intro' | 'interlude' | 'line'>(() => {
  if (props.lyricsState !== 'loaded' || !props.lyrics?.lines.length) return 'status';
  if (activeLineIndex.value < 0) return 'intro';
  const line = props.lyrics.lines[activeLineIndex.value];
  const next = props.lyrics.lines[activeLineIndex.value + 1];
  if (!next) return 'line';
  const lineEnd = line.end_ms ?? line.begin_ms;
  if (next.begin_ms - lineEnd >= INTERLUDE_GAP_MS && props.positionMs >= lineEnd) return 'interlude';
  return 'line';
});

const displayText = computed(() => {
  if (displayMode.value === 'intro') return [props.title, props.artist].filter(Boolean).join(' - ');
  if (displayMode.value === 'interlude') return '● ● ●';
  if (props.lyricsState === 'loading') return '正在搜索歌词...';
  if (props.lyricsState === 'notFound') return '暂无匹配歌词';
  if (props.lyricsState === 'error') return props.lyricsMessage || '歌词加载失败';
  return '';
});

</script>
