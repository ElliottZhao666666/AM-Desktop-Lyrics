<template>
  <SettingsView v-if="isSettingsWindow" />
  <UnlockButton v-else-if="isUnlockWindow" />
  <div v-else
    class="lyrics-window w-screen h-screen relative overflow-hidden"
    :class="{ 'is-interactive': isHovering && !controlsLocked }"
    @mouseenter="isHovering = true"
    @mouseleave="isHovering = false"
    @contextmenu.prevent
  >
    <template v-if="isHovering && !controlsLocked">
      <div v-for="edge in resizeEdges" :key="edge.direction" class="resize-edge" :class="edge.className" @mousedown.stop.prevent="startResize(edge.direction)" />
    </template>
    <ControlBar
      v-if="!controlsLocked"
      :show="isHovering"
      :title="mediaTitle" 
      :artist="mediaArtist"
      :is-playing="isPlaying"
      :position-ms="displayPositionMs"
      :duration-ms="durationMs"
      @prev="onPrev"
      @playPause="onPlayPause"
      @next="onNext"
      @lyricsEarlier="adjustLyricsOffset(-500)"
      @lyricsLater="adjustLyricsOffset(500)"
      @settings="onSettings"
      @toggleLock="setWindowLocked(true)"
      @close="onHide"
    />
    <LyricsBoard 
      :title="mediaTitle"
      :artist="mediaArtist"
      :is-playing="isPlaying"
      :position-ms="displayPositionMs"
      :lyrics="currentLyrics"
      :lyrics-state="lyricsState"
      :lyrics-message="lyricsMessage"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import ControlBar from './components/ControlBar.vue';
import LyricsBoard from './components/LyricsBoard.vue';
import SettingsView from './components/SettingsView.vue';
import UnlockButton from './components/UnlockButton.vue';
import type {
  AppSettings,
  LyricsDocument,
  LyricsLoadState,
  LyricsStatusPayload,
  MediaPositionPayload,
  MediaSnapshotPayload,
  MediaStatePayload,
  RuntimeLyricsTimeOffset,
} from './types';

const currentWindowLabel = getCurrentWindow().label;
const isSettingsWindow = currentWindowLabel === 'settings';
const isUnlockWindow = currentWindowLabel === 'unlock';

// ==========================================
// 响应式状态
// ==========================================
const isHovering = ref(false);
const mediaTitle = ref('等待 Apple Music 播放...');
const mediaArtist = ref('');
const isPlaying = ref(false);
const positionMs = ref(0);
const durationMs = ref(0);
const lyricsTimeOffsetMs = ref(0);
let lyricsTimeOffsetRevision = -1;
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
let unlistenLockState: UnlistenFn | null = null;
let unlistenSettings: UnlistenFn | null = null;
let unlistenLyricsTimeOffset: UnlistenFn | null = null;
let clockAnchorPositionMs = 0;
let clockAnchorTime = performance.now();

const CLOCK_SNAP_THRESHOLD_MS = 300;
const displayPositionMs = computed(() => {
  const adjusted = positionMs.value - lyricsTimeOffsetMs.value;
  return Math.max(0, durationMs.value > 0 ? Math.min(adjusted, durationMs.value) : adjusted);
});
type ResizeDirection = 'East' | 'North' | 'NorthEast' | 'NorthWest' | 'South' | 'SouthEast' | 'SouthWest' | 'West';
const resizeEdges: Array<{ direction: ResizeDirection; className: string }> = [
  { direction: 'North', className: 'resize-north' },
  { direction: 'South', className: 'resize-south' },
  { direction: 'West', className: 'resize-west' },
  { direction: 'East', className: 'resize-east' },
  { direction: 'NorthWest', className: 'resize-north-west' },
  { direction: 'NorthEast', className: 'resize-north-east' },
  { direction: 'SouthWest', className: 'resize-south-west' },
  { direction: 'SouthEast', className: 'resize-south-east' },
];

function applyTheme(css: string) {
  let style = document.querySelector<HTMLStyleElement>('#amdl-runtime-theme');
  if (!style) {
    style = document.createElement('style');
    style.id = 'amdl-runtime-theme';
    document.head.appendChild(style);
  }
  style.textContent = css;
}

function applyLyricsTimeOffset(payload: RuntimeLyricsTimeOffset) {
  if (payload.revision < lyricsTimeOffsetRevision) return;
  lyricsTimeOffsetRevision = payload.revision;
  lyricsTimeOffsetMs.value = payload.offsetMs;
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
  if (isSettingsWindow || isUnlockWindow) return;

  unlistenLockState = await listen<boolean>('lock-state-changed', (event) => {
    controlsLocked.value = event.payload;
    if (event.payload) isHovering.value = false;
  });
  try {
    controlsLocked.value = await invoke<boolean>('get_lock_state');
  } catch (error) {
    console.error('get_lock_state failed:', error);
  }

  unlistenLyricsTimeOffset = await listen<RuntimeLyricsTimeOffset>('lyrics-time-offset-changed', (event) => {
    applyLyricsTimeOffset(event.payload);
  });
  try {
    const settings = await invoke<AppSettings>('get_settings');
    applyTheme(settings.themeCss);
    applyLyricsTimeOffset(await invoke<RuntimeLyricsTimeOffset>('get_runtime_lyrics_time_offset'));
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
  unlistenLockState?.();
  unlistenSettings?.();
  unlistenLyricsTimeOffset?.();
  if (animationFrameId !== null) cancelAnimationFrame(animationFrameId);
});

function sameTrack(title: string, artist: string) {
  return title === mediaTitle.value && artist === mediaArtist.value;
}

async function startResize(direction: ResizeDirection) {
  try {
    await getCurrentWindow().startResizeDragging(direction);
  } catch (error) {
    console.error('window resize failed:', error);
  }
}

async function adjustLyricsOffset(deltaMs: number) {
  try {
    applyLyricsTimeOffset(await invoke<RuntimeLyricsTimeOffset>('adjust_runtime_lyrics_time_offset', { deltaMs }));
  } catch (error) {
    console.error('adjust_runtime_lyrics_time_offset failed:', error);
  }
}

async function setWindowLocked(locked: boolean) {
  try {
    controlsLocked.value = await invoke<boolean>('set_window_locked', { locked });
  } catch (error) {
    console.error('set_window_locked failed:', error);
  }
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

<style scoped>
.lyrics-window {
  background: transparent;
  transition: background-color 160ms ease;
}

.lyrics-window.is-interactive {
  background: rgb(0 0 0 / 10%);
}

.resize-edge {
  position: absolute;
  z-index: 100;
}

.resize-north,
.resize-south {
  right: 8px;
  left: 8px;
  height: 6px;
}

.resize-west,
.resize-east {
  top: 8px;
  bottom: 8px;
  width: 6px;
}

.resize-north { top: 0; cursor: n-resize; }
.resize-south { bottom: 0; cursor: s-resize; }
.resize-west { left: 0; cursor: w-resize; }
.resize-east { right: 0; cursor: e-resize; }

.resize-north-west,
.resize-north-east,
.resize-south-west,
.resize-south-east {
  width: 10px;
  height: 10px;
}

.resize-north-west { top: 0; left: 0; cursor: nw-resize; }
.resize-north-east { top: 0; right: 0; cursor: ne-resize; }
.resize-south-west { bottom: 0; left: 0; cursor: sw-resize; }
.resize-south-east { right: 0; bottom: 0; cursor: se-resize; }
</style>