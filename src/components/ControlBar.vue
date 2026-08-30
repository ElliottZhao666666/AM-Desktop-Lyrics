<template>
  <div
    class="absolute top-0 left-0 z-50 flex w-full flex-col bg-black/60 shadow-lg backdrop-blur-md transition-all duration-300"
    :class="show ? 'translate-y-0 opacity-100 pointer-events-auto' : '-translate-y-2 opacity-0 pointer-events-none'"
    @mousedown="startWindowDrag"
  >
    <div class="flex min-h-10 items-center justify-between gap-3 px-3 py-1">
      <div ref="trackViewport" class="min-w-0 flex-1 overflow-hidden whitespace-nowrap text-sm text-white">
        <div ref="trackText" class="inline-block" :style="trackTextStyle">
          <template v-if="title && artist"><strong class="font-bold">{{ title }}</strong><span class="text-white/70"> — {{ artist }}</span></template>
          <template v-else-if="title"><strong class="font-bold">{{ title }}</strong></template>
          <template v-else>等待播放...</template>
        </div>
      </div>
      <div class="flex flex-shrink-0 items-center gap-1 text-white">
        <span class="w-10 text-right text-xs tabular-nums text-white/60">{{ formatTime(positionMs) }}</span>
        <span class="w-10 text-right text-xs tabular-nums text-white/60">{{ formatTime(durationMs) }}</span>
        <button class="flex h-7 w-7 items-center justify-center transition-colors hover:text-pink-400 active:scale-90" @mousedown.stop @click="emit('prev')" title="上一曲"><SkipBack :size="16" /></button>
        <button class="flex h-7 w-7 items-center justify-center transition-colors hover:text-pink-400 active:scale-90" @mousedown.stop @click="emit('playPause')" title="播放/暂停"><Pause v-if="isPlaying" :size="17" /><Play v-else :size="17" /></button>
        <button class="flex h-7 w-7 items-center justify-center transition-colors hover:text-pink-400 active:scale-90" @mousedown.stop @click="emit('next')" title="下一曲"><SkipForward :size="16" /></button>
        <button class="flex h-7 w-7 items-center justify-center transition-colors hover:text-pink-400 active:scale-90" @mousedown.stop @click="emit('toggleLock')" :title="locked ? '解锁窗口拖动' : '锁定窗口拖动'"><Lock v-if="locked" :size="15" /><Unlock v-else :size="15" /></button>
        <button class="flex h-7 w-7 items-center justify-center text-white/70 transition-colors hover:text-white active:scale-90" @mousedown.stop @click="emit('settings')" title="设置"><Settings :size="16" /></button>
        <button class="flex h-7 w-7 items-center justify-center transition-colors hover:text-red-400 active:scale-90" @mousedown.stop @click="emit('close')" title="关闭"><X :size="17" /></button>
      </div>
    </div>
    <div class="h-0.5 w-full bg-white/15" aria-hidden="true">
      <div class="amdl-progress-fill h-full transition-[width] duration-300 ease-linear" :style="{ width: `${progressPercent}%` }"></div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Lock, Pause, Play, Settings, SkipBack, SkipForward, Unlock, X } from '@lucide/vue';

const props = defineProps<{
  show: boolean;
  title: string;
  artist: string;
  isPlaying: boolean;
  positionMs: number;
  durationMs: number;
  locked: boolean;
}>();
const emit = defineEmits(['prev', 'playPause', 'next', 'toggleLock', 'settings', 'close']);
const progressPercent = computed(() => props.durationMs > 0 ? Math.min(props.positionMs / props.durationMs * 100, 100) : 0);
const trackViewport = ref<HTMLElement | null>(null);
const trackText = ref<HTMLElement | null>(null);
const trackOverflow = ref(0);
const trackTextStyle = computed(() => trackOverflow.value > 0 ? {
  '--track-overflow': `-${trackOverflow.value}px`,
  animation: 'track-marquee 10s ease-in-out infinite',
} : undefined);
let resizeObserver: ResizeObserver | null = null;

function measureTrackText() {
  trackOverflow.value = Math.max(0, (trackText.value?.scrollWidth ?? 0) - (trackViewport.value?.clientWidth ?? 0));
}

onMounted(() => {
  resizeObserver = new ResizeObserver(measureTrackText);
  if (trackViewport.value) resizeObserver.observe(trackViewport.value);
  if (trackText.value) resizeObserver.observe(trackText.value);
  measureTrackText();
});
onBeforeUnmount(() => resizeObserver?.disconnect());
watch(() => [props.title, props.artist], () => nextTick(measureTrackText));
function formatTime(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  return `${Math.floor(totalSeconds / 60)}:${(totalSeconds % 60).toString().padStart(2, '0')}`;
}
async function startWindowDrag(event: MouseEvent) {
  if (event.button !== 0 || props.locked) return;
  try {
    await getCurrentWindow().startDragging();
  } catch (error) {
    console.error('window drag failed:', error);
  }
}
</script>

<style scoped>
@keyframes track-marquee {
  0%, 25% { transform: translateX(0); }
  45%, 70% { transform: translateX(var(--track-overflow)); }
  90%, 100% { transform: translateX(0); }
}
</style>
