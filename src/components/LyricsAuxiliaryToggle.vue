<template>
  <div ref="root" class="auxiliary-toggle" @mousedown.stop @focusout="onFocusOut" @keydown.esc.stop.prevent="closeMenu(true)">
    <button ref="toggle" type="button" class="auxiliary-button" :class="{ enabled: mode !== 'off' }" aria-label="歌词翻译与注音" :aria-pressed="mode !== 'off'" :aria-haspopup="both ? 'menu' : undefined" :aria-expanded="both ? menuOpen : undefined" :aria-controls="both ? menuId : undefined" :title="mode === 'translation' ? '正在显示翻译' : mode === 'pronunciation' ? '正在显示注音' : '打开歌词翻译或注音'" @click="activate" @keydown.down.prevent="openMenu">
      <Languages :size="16" />
    </button>
    <div v-if="menuOpen" :id="menuId" ref="menu" class="auxiliary-menu" :style="menuStyle" role="menu" aria-label="歌词翻译与注音选项" @keydown.down.prevent="moveFocus(1)" @keydown.up.prevent="moveFocus(-1)">
      <button v-for="item in items" :key="item.mode" type="button" role="menuitemcheckbox" :aria-checked="enabled[item.mode]" :class="{ selected: enabled[item.mode] }" :title="enabled[item.mode] ? '再次点击可关闭；两项都开启时优先显示翻译' : '本次运行持续开启；两项都开启时优先显示翻译'" @click="select(item.mode)">{{ item.label }}</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import { Languages } from '@lucide/vue';
import type { LyricsAuxiliaryMode } from '../types';
import type { AuxiliaryAvailability } from '../localization';
const props = defineProps<{ translation: boolean; pronunciation: boolean; mode: LyricsAuxiliaryMode; enabled: AuxiliaryAvailability; visible: boolean }>();
const emit = defineEmits<{ change: [change: { mode: Exclude<LyricsAuxiliaryMode, 'off'>; enabled: boolean }] }>();
const root = ref<HTMLElement | null>(null), toggle = ref<HTMLButtonElement | null>(null), menu = ref<HTMLElement | null>(null);
const menuOpen = ref(false), menuStyle = ref<{ top?: string; left?: string; maxHeight?: string }>({});
const menuId = `lyrics-auxiliary-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`;
const both = computed(() => props.translation && props.pronunciation);
const items: { mode: Exclude<LyricsAuxiliaryMode, 'off'>; label: string }[] = [
  { mode: 'translation', label: '打开翻译' }, { mode: 'pronunciation', label: '打开注音' },
];
function closeMenu(focus = false) { menuOpen.value = false; if (focus) toggle.value?.focus(); }
function placeMenu() {
  if (!root.value || !toggle.value || !menu.value) return;
  const origin = root.value.getBoundingClientRect(), button = toggle.value.getBoundingClientRect();
  const width = Math.min(158, Math.max(0, window.innerWidth - 8));
  const height = Math.min(menu.value.scrollHeight, Math.max(0, window.innerHeight - 8));
  const left = Math.max(4, Math.min(button.right - width, window.innerWidth - width - 4));
  const top = Math.max(4, Math.min(button.bottom + 4, window.innerHeight - height - 4));
  menuStyle.value = { left: `${left - origin.left}px`, top: `${top - origin.top}px`, maxHeight: `${Math.max(0, window.innerHeight - 8)}px` };
}
async function openMenu() {
  if (!both.value || !props.visible) return;
  menuOpen.value = true;
  await nextTick(); placeMenu();
  menu.value?.querySelector<HTMLButtonElement>('button[aria-checked="true"]')?.focus();
  if (!menu.value?.contains(document.activeElement)) menu.value?.querySelector('button')?.focus();
}
function activate() {
  if (both.value) { if (menuOpen.value) closeMenu(); else void openMenu(); return; }
  const next = props.translation ? 'translation' : 'pronunciation';
  emit('change', { mode: next, enabled: !props.enabled[next] });
}
function select(mode: Exclude<LyricsAuxiliaryMode, 'off'>) { emit('change', { mode, enabled: !props.enabled[mode] }); closeMenu(true); }
function moveFocus(direction: number) {
  const buttons = [...menu.value?.querySelectorAll<HTMLButtonElement>('button') ?? []];
  const index = buttons.findIndex(button => button === document.activeElement);
  buttons[(index + direction + buttons.length) % buttons.length]?.focus();
}
function onFocusOut(event: FocusEvent) { if (!root.value?.contains(event.relatedTarget as Node | null)) closeMenu(); }
function onPointerDown(event: PointerEvent) { if (!root.value?.contains(event.target as Node)) closeMenu(); }
function onBlur() { closeMenu(); }
watch(() => [props.visible, props.translation, props.pronunciation], () => { if (!props.visible || !both.value) closeMenu(); });
onMounted(() => { document.addEventListener('pointerdown', onPointerDown, true); window.addEventListener('blur', onBlur); window.addEventListener('resize', placeMenu); });
onUnmounted(() => { document.removeEventListener('pointerdown', onPointerDown, true); window.removeEventListener('blur', onBlur); window.removeEventListener('resize', placeMenu); });
</script>

<style scoped>
.auxiliary-toggle { position: relative; display: flex; flex: 0 0 auto; }
.auxiliary-button { display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; padding: 0; border: 0; background: transparent; color: inherit; cursor: pointer; transition: color .15s; }
.auxiliary-button:hover, .auxiliary-button.enabled { color: #f472b6; }
.auxiliary-button:focus-visible, .auxiliary-menu button:focus-visible { outline: 2px solid #f472b6; outline-offset: -2px; border-radius: 4px; }
.auxiliary-menu { position: absolute; z-index: 80; box-sizing: border-box; width: min(158px, calc(100vw - 8px)); padding: 5px; overflow: auto; border: 1px solid #55515b; border-radius: 6px; background: #17151b; box-shadow: 0 4px 16px #0006; }
.auxiliary-menu button { display: block; width: 100%; min-height: 30px; padding: 5px 10px; border: 0; border-radius: 4px; background: transparent; color: #d6d3da; text-align: left; font-size: 12px; cursor: pointer; }
.auxiliary-menu button:hover, .auxiliary-menu button:focus { background: #ffffff12; color: white; }
.auxiliary-menu button.selected { color: #f472b6; background: #f472b617; }
</style>
