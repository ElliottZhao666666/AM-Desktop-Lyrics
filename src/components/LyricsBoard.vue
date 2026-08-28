<template>
  <div class="w-full h-full flex flex-col justify-center items-center bg-transparent text-white select-none px-6 pointer-events-none" data-tauri-drag-region>
    <!-- 歌曲信息行 -->
    <div class="text-base font-semibold mb-1 tracking-wide drop-shadow-md text-white/80 transition-opacity duration-500"
         :class="title && isPlaying ? 'opacity-100' : 'opacity-60'"
    >
      <template v-if="title && artist">
        {{ title }} — {{ artist }}
      </template>
      <template v-else>
        等待 Apple Music 播放...
      </template>
    </div>

    <!-- 歌词展示 -->
    <div class="text-3xl font-black bg-clip-text text-transparent bg-gradient-to-r from-pink-400 via-fuchsia-400 to-purple-500 drop-shadow-lg transition-all duration-300"
         :class="isPlaying ? 'opacity-100 scale-100' : 'opacity-50 scale-95'"
    >
      <template v-if="lyrics.length === 0">
        ♪ {{ isPlaying ? '正在加载歌词...' : '已暂停' }} ♪
      </template>
      <template v-else>
        {{ currentLyricText }}
      </template>
    </div>

    <!-- 进度条 -->
    <div v-if="durationMs > 0" class="w-full max-w-md mt-3">
      <div class="w-full h-1 bg-white/20 rounded-full overflow-hidden">
        <div 
          class="h-full bg-gradient-to-r from-pink-400 to-purple-500 rounded-full transition-all duration-500 ease-linear"
          :style="{ width: progressPercent + '%' }"
        ></div>
      </div>
      <div class="flex justify-between text-xs text-white/50 mt-1">
        <span>{{ formatTime(positionMs) }}</span>
        <span>{{ formatTime(durationMs) }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { LyricLinePayload } from '../App.vue';

const props = defineProps<{
  title: string;
  artist: string;
  isPlaying: boolean;
  positionMs: number;
  durationMs: number;
  lyrics: LyricLinePayload[];
}>();

/** 计算播放进度百分比 */
const progressPercent = computed(() => {
  if (props.durationMs <= 0) return 0;
  return Math.min((props.positionMs / props.durationMs) * 100, 100);
});

/** 计算当前应该高亮显示的歌词 */
const currentLyricText = computed(() => {
  if (!props.lyrics || props.lyrics.length === 0) return '';
  
  // 找到最后一句 time <= positionMs 的歌词
  let current = props.lyrics[0];
  for (const line of props.lyrics) {
    if (line.time <= props.positionMs) {
      current = line;
    } else {
      break; // 因为时间是有序的，找到第一个大于当前时间的就可以提前结束
    }
  }
  return current.text;
});

/** 将毫秒格式化为 mm:ss */
function formatTime(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}
</script>
