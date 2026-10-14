<template>
  <div ref="root" class="lyrics-refresh" @focusout="onFocusOut" @keydown.esc.stop.prevent="closeMenu(true)">
    <button class="refresh-main" :disabled="disabled" title="按照当前渠道设置，重新读取歌词池并匹配获取歌词" @click="request('all')"><RefreshCw :size="16" :class="{ spinning: busy }" />重新获取歌词</button>
    <button ref="toggle" class="refresh-toggle" :disabled="disabled" aria-label="选择重新获取渠道" aria-haspopup="menu" :aria-expanded="menuOpen" :aria-controls="menuId" title="指定本次获取渠道" @click="toggleMenu" @keydown.down.prevent="openMenu"><ChevronDown :size="14" /></button>
    <div v-if="menuOpen" :id="menuId" ref="menu" class="refresh-menu" role="menu" aria-label="重新获取渠道" @keydown.down.prevent="moveFocus(1)" @keydown.up.prevent="moveFocus(-1)">
      <button v-for="item in items" :key="item.source" role="menuitem" type="button" :title="item.title" @click="request(item.source)">{{ item.label }}</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import { ChevronDown, RefreshCw } from '@lucide/vue';
import type { LyricsRefetchSource } from '../types';
const props = defineProps<{disabled: boolean; busy: boolean}>();
const emit = defineEmits<{refresh: [source: LyricsRefetchSource]}>();
const items: {source: LyricsRefetchSource; label: string; title: string}[] = [
  {source: 'appleMusicApi', label: '重新获取 AM API', title: '本次仅使用 Apple Music 官方 API 获取歌词'},
  {source: 'appleMusicCache', label: '重新获取 缓存', title: '本次仅扫描 Apple Music 本地缓存，不读取 AMDL 缓存池'},
  {source: 'qqMusic', label: '重新获取 Q音', title: '本次仅使用 QQ 音乐获取歌词'},
];
const root = ref<HTMLElement | null>(null), toggle = ref<HTMLButtonElement | null>(null), menu = ref<HTMLElement | null>(null);
const menuOpen = ref(false), menuId = `lyrics-refresh-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`;
function closeMenu(restoreFocus = false) { menuOpen.value = false; if (restoreFocus) toggle.value?.focus(); }
async function openMenu() { if (props.disabled) return; menuOpen.value = true; await nextTick(); menu.value?.querySelector('button')?.focus(); }
function toggleMenu() { if (menuOpen.value) closeMenu(); else void openMenu(); }
function request(source: LyricsRefetchSource) { if (props.disabled) return; closeMenu(); emit('refresh', source); }
function moveFocus(direction: number) {
  const buttons = Array.from(menu.value?.querySelectorAll('button') ?? []);
  if (!buttons.length) return;
  const index = buttons.findIndex(button => button === document.activeElement);
  buttons[(index + direction + buttons.length) % buttons.length]?.focus();
}
function onFocusOut(event: FocusEvent) { if (!root.value?.contains(event.relatedTarget as Node | null)) closeMenu(); }
function onPointerDown(event: PointerEvent) { if (!root.value?.contains(event.target as Node)) closeMenu(); }
function onBlur() { closeMenu(); }
watch(() => props.disabled, disabled => { if (disabled) closeMenu(); });
onMounted(() => { document.addEventListener('pointerdown', onPointerDown, true); window.addEventListener('blur', onBlur); });
onUnmounted(() => { document.removeEventListener('pointerdown', onPointerDown, true); window.removeEventListener('blur', onBlur); });
</script>

<style scoped>
.lyrics-refresh { position: relative; display: inline-flex; height: 32px; flex: 0 0 auto; }
.refresh-main, .refresh-toggle { display: inline-flex; align-items: center; justify-content: center; gap: 6px; height: 32px; padding: 0 11px; border: 1px solid #ceced4; background: #fff; color: #55555e; cursor: pointer; font: 12px "Segoe UI", "Microsoft YaHei", sans-serif; }
.refresh-main { border-radius: 5px 0 0 5px; }.refresh-toggle { width: 30px; padding: 0; border-left: 0; border-radius: 0 5px 5px 0; }
.refresh-main:hover:not(:disabled), .refresh-toggle:hover:not(:disabled) { color: #bd2c76; background: #fbeef5; }.refresh-main:focus-visible, .refresh-toggle:focus-visible, .refresh-menu button:focus-visible { outline: 2px solid #d13c86; outline-offset: -2px; }
button:disabled { opacity: .45; cursor: default; }.refresh-menu { position: absolute; z-index: 20; right: 0; top: calc(100% + 6px); width: 190px; padding: 5px; border: 1px solid #dcdce3; border-radius: 7px; box-shadow: 0 5px 18px #24242a1c; background: #fff; }
.refresh-menu button { display: block; width: 100%; min-height: 34px; border: 0; border-radius: 4px; padding: 0 10px; background: transparent; color: #55555e; text-align: left; font: 12px "Segoe UI", "Microsoft YaHei", sans-serif; cursor: pointer; }.refresh-menu button:hover, .refresh-menu button:focus { color: #bd2c76; background: #fbeef5; }
@keyframes spin { to { transform: rotate(360deg); } }.spinning { animation: spin 1s linear infinite; }
</style>
