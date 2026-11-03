<template>
  <div class="auxiliary-toggle" @mousedown.stop>
    <button v-if="translation" type="button" class="auxiliary-button" :class="{ enabled: mode === 'translation' }" aria-label="歌词翻译" :aria-pressed="mode === 'translation'" title="翻译" @click="emit('change', { mode: 'translation', enabled: mode !== 'translation' })"><Languages :size="16" /></button>
    <button v-if="pronunciation" type="button" class="auxiliary-button" :class="{ enabled: mode === 'pronunciation' }" aria-label="歌词注音" :aria-pressed="mode === 'pronunciation'" title="注音" @click="emit('change', { mode: 'pronunciation', enabled: mode !== 'pronunciation' })"><Speech :size="16" /></button>
  </div>
</template>
<script setup lang="ts">
import { Languages, Speech } from '@lucide/vue';
import type { LyricsAuxiliaryMode } from '../types';
import type { AuxiliaryAvailability } from '../localization';
defineProps<{ translation: boolean; pronunciation: boolean; mode: LyricsAuxiliaryMode; enabled: AuxiliaryAvailability; visible: boolean }>();
const emit = defineEmits<{ change: [change: { mode: Exclude<LyricsAuxiliaryMode, 'off'>; enabled: boolean }] }>();
</script>
<style scoped>
.auxiliary-toggle { display: flex; flex: 0 0 auto; gap: 4px; }
.auxiliary-button { display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; padding: 0; border: 0; background: transparent; color: inherit; cursor: pointer; transition: color .15s; }
.auxiliary-button:hover, .auxiliary-button.enabled { color: #f472b6; }
.auxiliary-button:focus-visible { outline: 2px solid #f472b6; outline-offset: -2px; border-radius: 4px; }
</style>
