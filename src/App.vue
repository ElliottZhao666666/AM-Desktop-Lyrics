<template>
  <div 
    class="w-screen h-screen relative overflow-hidden bg-transparent"
    @mouseenter="isHovering = true"
    @mouseleave="isHovering = false"
    @contextmenu.prevent
  >
    <ControlBar 
      :show="isHovering" 
      :title="mediaTitle" 
      :artist="mediaArtist"
      :is-playing="isPlaying"
      @prev="onPrev"
      @playPause="onPlayPause"
      @next="onNext"
      @settings="onSettings"
      @close="onClose"
    />
    <LyricsBoard 
      :title="mediaTitle"
      :artist="mediaArtist"
      :is-playing="isPlaying"
      :position-ms="positionMs"
      :duration-ms="durationMs"
      :lyrics="currentLyrics"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import ControlBar from './components/ControlBar.vue';
import LyricsBoard from './components/LyricsBoard.vue';

// ==========================================
// 响应式状态
// ==========================================
const isHovering = ref(false);
const mediaTitle = ref('等待 Apple Music 播放...');
const mediaArtist = ref('');
const isPlaying = ref(false);
const positionMs = ref(0);
const durationMs = ref(0);
const currentLyrics = ref<LyricLinePayload[]>([]);

// 事件注销句柄
let unlistenMediaState: UnlistenFn | null = null;
let unlistenMediaPosition: UnlistenFn | null = null;
let unlistenLyricsLoaded: UnlistenFn | null = null;

// ==========================================
// 媒体状态事件载荷类型定义
// ==========================================
interface MediaStatePayload {
  title: string;
  artist: string;
  playback_state: 'Playing' | 'Paused' | 'Stopped' | 'Unknown';
}

interface MediaPositionPayload {
  position_ms: number;
  duration_ms: number;
}

interface MediaSnapshotPayload {
  media: MediaStatePayload;
  position: MediaPositionPayload;
}

export interface LyricLinePayload {
  time: number;
  text: string;
}

function applyMediaState(payload: MediaStatePayload) {
  const trackChanged = payload.title !== mediaTitle.value || payload.artist !== mediaArtist.value;
  if (trackChanged) {
    currentLyrics.value = [];
    positionMs.value = 0;
    durationMs.value = 0;
  }

  mediaTitle.value = payload.title;
  mediaArtist.value = payload.artist;
  isPlaying.value = payload.playback_state === 'Playing';
}

// ==========================================
// 生命周期
// ==========================================
onMounted(async () => {
  // 监听 Rust 端广播的媒体状态变更事件
  unlistenMediaState = await listen<MediaStatePayload>('media-state-changed', (event) => {
    console.log('[IPC] media-state-changed:', event.payload);
    applyMediaState(event.payload);
  });

  // 监听 Rust 端广播的播放进度变更事件
  unlistenMediaPosition = await listen<MediaPositionPayload>('media-position-changed', (event) => {
    const payload = event.payload;
    positionMs.value = payload.position_ms;
    durationMs.value = payload.duration_ms;
  });

  // 监听假歌词加载完成事件
  unlistenLyricsLoaded = await listen<LyricLinePayload[]>('lyrics-loaded', (event) => {
    console.log('[IPC] lyrics-loaded:', event.payload);
    currentLyrics.value = event.payload;
  });

  // 监听全部就绪后主动读取一次快照，补回 WebView 挂载前可能错过的事件。
  try {
    const snapshot = await invoke<MediaSnapshotPayload>('get_media_snapshot');
    applyMediaState(snapshot.media);
    positionMs.value = snapshot.position.position_ms;
    durationMs.value = snapshot.position.duration_ms;
  } catch (error) {
    console.error('get_media_snapshot failed:', error);
  }
});

onUnmounted(() => {
  unlistenMediaState?.();
  unlistenMediaPosition?.();
  unlistenLyricsLoaded?.();
});

// ==========================================
// 控制按钮事件处理
// ==========================================
const onPrev = async () => {
  console.log('[CMD] prev_track');
  try {
    await invoke('prev_track');
  } catch (e) {
    console.error('prev_track failed:', e);
  }
};

const onPlayPause = async () => {
  console.log('[CMD] play_pause');
  try {
    await invoke('play_pause');
  } catch (e) {
    console.error('play_pause failed:', e);
  }
};

const onNext = async () => {
  console.log('[CMD] next_track');
  try {
    await invoke('next_track');
  } catch (e) {
    console.error('next_track failed:', e);
  }
};

const onSettings = () => {
  console.log('[UI] Open settings');
};

const onClose = async () => {
  console.log('[UI] Close application');
  await getCurrentWindow().close();
};
</script>