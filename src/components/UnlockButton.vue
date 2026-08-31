<template>
  <div class="unlock-surface">
    <button class="unlock-button" title="解锁歌词窗口" aria-label="解锁歌词窗口" @click="unlock">
      <LockKeyhole :size="17" />
    </button>
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { LockKeyhole } from '@lucide/vue';

async function unlock() {
  try {
    await invoke('set_window_locked', { locked: false });
  } catch (error) {
    console.error('unlock lyrics window failed:', error);
  }
}
</script>

<style scoped>
.unlock-surface {
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 4px;
  box-sizing: border-box;
}
.unlock-button {
  width: 34px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid #ffffff38;
  border-radius: 6px;
  background: #17171dd9;
  color: #f7f7f9;
  box-shadow: 0 2px 8px #0000004d;
  opacity: 0;
  cursor: pointer;
  transition: opacity 120ms ease, color 120ms ease, transform 120ms ease;
}
.unlock-surface:hover .unlock-button,
.unlock-button:focus-visible {
  opacity: 1;
}
.unlock-button:hover { color: #fb7185; }
.unlock-button:active { transform: scale(.94); }
</style>
