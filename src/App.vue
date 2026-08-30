<template>
  <SettingsView v-if="isSettingsWindow" />
  <div v-else
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
      :position-ms="positionMs"
      :duration-ms="durationMs"
      :locked="controlsLocked"
      @prev="onPrev"
      @playPause="onPlayPause"
      @next="onNext"
      @settings="onSettings"
      @toggleLock="controlsLocked = !controlsLocked"
      @close="onHide"
    />
    <LyricsBoard 
      :title="mediaTitle"
      :artist="mediaArtist"
      :is-playing="isPlaying"
      :position-ms="positionMs"
      :lyrics="currentLyrics"
      :lyrics-state="lyricsState"
      :lyrics-message="lyricsMessage"
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
import SettingsView from './components/SettingsView.vue';
import type {
  AppSettings,
  LyricsDocument,
  LyricsLoadState,
  LyricsStatusPayload,
  MediaPositionPayload,
  MediaSnapshotPayload,
  MediaStatePayload,
} from './types';

const isSettingsWindow = getCurrentWindow().label === 'settings';

// ==========================================
// 响应式状态
// ==========================================
const isHovering = ref(false);
const mediaTitle = ref('等待 Apple Music 播放...');
const mediaArtist = ref('');
const isPlaying = ref(false);
const positionMs = ref(0);
const durationMs = ref(0);
const currentLyrics = ref<LyricsDocument | null>(null);
const lyricsState = ref<LyricsLoadState>('loading');
const lyricsMessage = ref<string | null>(null);
const controlsLocked = ref(false);

// 事件注销句柄
let unlistenMediaState: UnlistenFn | null = null;
let unlistenMediaPosition: UnlistenFn | null = null;
let unlistenLyricsLoaded: UnlistenFn | null = null;
let unlistenLyricsStatus: UnlistenFn | null = null;
let animationFrameId: number | null = null;
let unlistenTrayLock: UnlistenFn | null = null;
let unlistenSettings: UnlistenFn | null = null;
let clockAnchorPositionMs = 0;
let clockAnchorTime = performance.now();

const CLOCK_SNAP_THRESHOLD_MS = 300;

function applyTheme(css: string) {
  let style = document.querySelector<HTMLStyleElement>('#amdl-runtime-theme');
  if (!style) {
    style = document.createElement('style');
    style.id = 'amdl-runtime-theme';
    document.head.appendChild(style);
  }
  style.textContent = css;
}

function interpolatedPosition(now = performance.now()) {
  const elapsed = isPlaying.value ? Math.max(0, now - clockAnchorTime) : 0;
  const estimated = clockAnchorPositionMs + elapsed;
  return durationMs.value > 0 ? Math.min(estimated, durationMs.value) : estimated;
}

function applyAuthoritativePosition(payload: MediaPositionPayload) {
  const now = performance.now();
  const predicted = interpolatedPosition(now);
  const drift = payload.position_ms - predicted;
  const corrected = isPlaying.value && Math.abs(drift) <= CLOCK_SNAP_THRESHOLD_MS
    ? predicted + drift * 0.35
    : payload.position_ms;

  durationMs.value = payload.duration_ms;
  clockAnchorPositionMs = Math.max(0, corrected);
  clockAnchorTime = now;
  positionMs.value = clockAnchorPositionMs;
}

function updateInterpolatedPosition(now: number) {
  if (isPlaying.value) positionMs.value = interpolatedPosition(now);
  animationFrameId = requestAnimationFrame(updateInterpolatedPosition);
}

// ==========================================
// 媒体状态事件载荷类型定义
// ==========================================
function applyMediaState(payload: MediaStatePayload) {
  const trackChanged = payload.track.title !== mediaTitle.value || payload.track.artist !== mediaArtist.value;
  const nextIsPlaying = payload.playback_state === 'Playing';
  const now = performance.now();
  if (trackChanged) {
    currentLyrics.value = null;
    lyricsState.value = 'loading';
    lyricsMessage.value = null;
    positionMs.value = 0;
    durationMs.value = 0;
    clockAnchorPositionMs = 0;
    clockAnchorTime = now;
  } else if (nextIsPlaying !== isPlaying.value) {
    clockAnchorPositionMs = interpolatedPosition(now);
    clockAnchorTime = now;
    positionMs.value = clockAnchorPositionMs;
  }

  mediaTitle.value = payload.track.title;
  mediaArtist.value = payload.track.artist;
  isPlaying.value = nextIsPlaying;
}

// ==========================================
// 生命周期
// ==========================================
onMounted(async () => {
  if (isSettingsWindow) return;

  try {
    const settings = await invoke<AppSettings>('get_settings');
    applyTheme(settings.themeCss);
  } catch (error) {
    console.error('get_settings failed:', error);
  }
  unlistenSettings = await listen<AppSettings>('settings-changed', (event) => applyTheme(event.payload.themeCss));

  // 监听 Rust 端广播的媒体状态变更事件
  unlistenMediaState = await listen<MediaStatePayload>('media-state-changed', (event) => {
    console.log('[IPC] media-state-changed:', event.payload);
    applyMediaState(event.payload);
  });

  // 监听 Rust 端广播的播放进度变更事件
  unlistenMediaPosition = await listen<MediaPositionPayload>('media-position-changed', (event) => {
    applyAuthoritativePosition(event.payload);
  });

  // 监听歌词加载完成事件
  unlistenLyricsLoaded = await listen<LyricsDocument>('lyrics-loaded', (event) => {
    console.log('[IPC] lyrics-loaded:', event.payload);
    if (sameTrack(event.payload.track.title, event.payload.track.artist)) currentLyrics.value = event.payload;
  });

  unlistenLyricsStatus = await listen<LyricsStatusPayload>('lyrics-status-changed', (event) => {
    if (!sameTrack(event.payload.track.title, event.payload.track.artist)) return;
    lyricsState.value = event.payload.state;
    lyricsMessage.value = event.payload.message;
  });
  unlistenTrayLock = await listen('tray-toggle-lock', () => {
    controlsLocked.value = !controlsLocked.value;
  });

  // 监听全部就绪后主动读取一次快照，补回 WebView 挂载前可能错过的事件。
  try {
    const snapshot = await invoke<MediaSnapshotPayload>('get_media_snapshot');
    applyMediaState(snapshot.media);
    applyAuthoritativePosition(snapshot.position);
  } catch (error) {
    console.error('get_media_snapshot failed:', error);
  }

  animationFrameId = requestAnimationFrame(updateInterpolatedPosition);
});

onUnmounted(() => {
  unlistenMediaState?.();
  unlistenMediaPosition?.();
  unlistenLyricsLoaded?.();
  unlistenLyricsStatus?.();
  unlistenTrayLock?.();
  unlistenSettings?.();
  if (animationFrameId !== null) cancelAnimationFrame(animationFrameId);
});

function sameTrack(title: string, artist: string) {
  return title === mediaTitle.value && artist === mediaArtist.value;
}

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

const onSettings = async () => {
  console.log('[UI] Settings requested');
  try { await invoke('open_settings'); } catch (error) { console.error('open_settings failed:', error); }
};

const onHide = async () => {
  console.log('[UI] Hide lyrics window');
  await getCurrentWindow().hide();
};
</script>