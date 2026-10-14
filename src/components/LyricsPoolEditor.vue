<template>
  <main class="pool-editor-shell">
    <header class="editor-heading"><div><p>AMDL · {{ pool === 'cache' ? '缓存池' : '手动匹配池' }}</p><h1>{{ recordId ? '编辑歌词' : '添加歌词' }}</h1></div><span class="editor-type" v-if="validation">{{ validation.timing === 'word' ? '逐字' : '逐行' }} · {{ validation.lineCount }} 行</span></header>
    <form class="pool-editor-form" @submit.prevent="saveEntry">
      <fieldset :disabled="loading || saving" class="metadata-fields">
        <label>歌名<input v-model="draft.title" aria-label="歌名" autocomplete="off" maxlength="256" /></label>
        <label>艺人<input v-model="draft.artist" aria-label="艺人" autocomplete="off" maxlength="256" /></label>
        <label>专辑<input v-model="draft.album" aria-label="专辑" autocomplete="off" maxlength="256" /></label>
        <label>Apple Music 歌曲 ID<div class="id-field"><input v-model="draft.appleMusicId" aria-label="Apple Music 歌曲 ID" placeholder="可留空" inputmode="numeric" autocomplete="off" maxlength="32" /><button type="button" title="根据歌名、艺人和专辑获取 Apple Music ID" aria-label="获取 Apple Music 歌曲 ID" :disabled="lookupBusy || !draft.title.trim() || !draft.artist.trim()" @click="resolveId"><Search :size="15" :class="{ spinning: lookupBusy }" />{{ lookupBusy ? '获取中' : '获取' }}</button></div></label>
      </fieldset>
      <label class="lyrics-field">歌词内容（支持LRC、TTML 逐字、TTML逐行歌词直接粘贴解析）<textarea v-model="draft.lyricsText" aria-label="歌词内容（支持LRC、TTML 逐字、TTML逐行歌词直接粘贴解析）<" :disabled="loading || saving" spellcheck="false" placeholder="粘贴带时间戳的 TTML 或 LRC 歌词…" /></label>
      <div class="validation-status" role="status" :class="{ invalid: !!validationError, valid: !!validation }"><CheckCircle2 v-if="validation" :size="14" /><AlertCircle v-else-if="validationError" :size="14" /><span>{{ validating ? '正在检查歌词格式…' : validationError || (validation ? '格式有效，保存时将重新解析歌词时间轴。' : '支持 TTML 逐字、TTML 逐行和 LRC 歌词。') }}</span></div>
      <footer class="editor-footer"><p :class="{ error: !!error }" role="status">{{ error || notice || lookupNotice }}</p><button class="editor-button secondary" type="button" :disabled="saving" @click="closeEditor">关闭</button><button class="editor-button primary" type="submit" :disabled="!canSave"><Save :size="15" />{{ saving ? '保存中…' : '保存' }}</button></footer>
    </form>
  </main>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ask } from '@tauri-apps/plugin-dialog';
import { AlertCircle, CheckCircle2, Save, Search } from '@lucide/vue';
import type { LyricsPoolDetail, LyricsPoolKind, LyricsPoolValidation, TrackIdentity } from '../types';

const params = new URLSearchParams(window.location.search);
const pool: LyricsPoolKind = params.get('pool') === 'cache' ? 'cache' : 'manual';
const recordId = ref<string | null>(params.get('id'));
const revision = ref<string | null>(null);
const draft = reactive({title: '', artist: '', album: '', appleMusicId: '', lyricsText: ''});
const loading = ref(!!recordId.value), saving = ref(false), lookupBusy = ref(false), validating = ref(false);
const error = ref(''), notice = ref(''), lookupNotice = ref(''), validationError = ref('');
const validation = ref<LyricsPoolValidation | null>(null);
const baseline = ref(JSON.stringify(draft));
const dirty = computed(() => JSON.stringify(draft) !== baseline.value);
const canSave = computed(() => pool === 'manual' && !loading.value && !saving.value && !lookupBusy.value && !validating.value && !!validation.value && !!draft.title.trim() && !!draft.artist.trim() && (!recordId.value || !!revision.value));
let disposed = false, validationGeneration = 0, lookupGeneration = 0, validationTimer: ReturnType<typeof setTimeout> | null = null;
let unlistenClose: (() => void) | null = null, closingApproved = false, confirmingClose = false;
const track = (): TrackIdentity => ({title: draft.title.trim(), artist: draft.artist.trim(), album: draft.album.trim() || null});

watch(() => draft.lyricsText, () => {
  const generation = ++validationGeneration;
  if (validationTimer) clearTimeout(validationTimer);
  validation.value = null; validationError.value = ''; validating.value = !!draft.lyricsText.trim(); notice.value = '';
  if (!validating.value) return;
  validationTimer = setTimeout(async () => {
    try {
      const result = await invoke<LyricsPoolValidation>('validate_pool_lyrics', {lyricsText: draft.lyricsText});
      if (generation === validationGeneration && !disposed) validation.value = result;
    } catch (reason) { if (generation === validationGeneration && !disposed) validationError.value = String(reason); }
    finally { if (generation === validationGeneration && !disposed) validating.value = false; }
  }, 350);
}, {flush: 'sync'});
watch(() => [draft.title, draft.artist, draft.album], () => { lookupGeneration++; lookupBusy.value = false; lookupNotice.value = ''; });

async function loadEntry() {
  if (!recordId.value) return;
  const detail = await invoke<LyricsPoolDetail>('get_lyrics_pool_entry', {pool, id: recordId.value});
  if (disposed) return;
  Object.assign(draft, {title: detail.entry.track.title, artist: detail.entry.track.artist, album: detail.entry.track.album || '', appleMusicId: detail.entry.appleMusicId || '', lyricsText: detail.lyricsText});
  revision.value = detail.entry.revision;
  baseline.value = JSON.stringify(draft);
}
onMounted(async () => {
  try {
    unlistenClose = await getCurrentWindow().onCloseRequested(async event => {
      if (saving.value) { event.preventDefault(); return; }
      if (!dirty.value || closingApproved) return;
      event.preventDefault();
      if (confirmingClose) return;
      confirmingClose = true;
      try { if (await ask('歌词修改尚未保存，确定关闭吗？', {title: '关闭歌词编辑', kind: 'warning', okLabel: '放弃修改并关闭', cancelLabel: '继续编辑'})) { closingApproved = true; await getCurrentWindow().close(); } }
      finally { confirmingClose = false; }
    });
    await loadEntry();
  } catch (reason) { error.value = String(reason); }
  finally { loading.value = false; }
});
onUnmounted(() => { disposed = true; validationGeneration++; lookupGeneration++; if (validationTimer) clearTimeout(validationTimer); unlistenClose?.(); });

async function resolveId() {
  if (lookupBusy.value) return;
  const query = track(), generation = ++lookupGeneration, initialId = draft.appleMusicId;
  lookupBusy.value = true; error.value = ''; lookupNotice.value = '';
  try {
    const id = await invoke<string>('resolve_pool_apple_music_id', {track: query});
    if (disposed || generation !== lookupGeneration || draft.appleMusicId !== initialId) return;
    draft.appleMusicId = id; lookupNotice.value = '已获取 Apple Music 歌曲 ID';
  } catch (reason) { if (!disposed && generation === lookupGeneration) error.value = String(reason); }
  finally { if (!disposed && generation === lookupGeneration) lookupBusy.value = false; }
}
async function saveEntry() {
  if (!canSave.value) return;
  saving.value = true; error.value = ''; notice.value = '';
  try {
    const id = await invoke<string>('save_lyrics_pool_entry', {draft: {pool, id: recordId.value, revision: revision.value, track: track(), appleMusicId: draft.appleMusicId, lyricsText: draft.lyricsText}});
    recordId.value = id; baseline.value = JSON.stringify(draft);
    await loadEntry(); notice.value = '歌词已解析并保存';
  } catch (reason) { error.value = String(reason); }
  finally { saving.value = false; }
}
async function closeEditor() { await getCurrentWindow().close(); }
</script>

<style scoped>
.pool-editor-shell { width: 100vw; height: 100vh; display: flex; flex-direction: column; padding: 24px 28px 20px; box-sizing: border-box; background: #f4f4f6; color: #33333a; font-family: "Segoe UI", "Microsoft YaHei", sans-serif; }
.editor-heading { display: flex; align-items: center; justify-content: space-between; padding-bottom: 18px; }.editor-heading p { margin: 0 0 5px; color: #bd2c76; font-size: 11px; }.editor-heading h1 { margin: 0; font-size: 21px; font-weight: 650; }.editor-type { background: #f8e5f0; color: #bd2c76; padding: 5px 9px; border-radius: 5px; font-size: 11px; }
.pool-editor-form { flex: 1; min-height: 0; display: flex; flex-direction: column; }.metadata-fields { display: grid; grid-template-columns: 1fr 1fr; gap: 12px 16px; border: 0; padding: 0; margin: 0 0 15px; min-width: 0; }.metadata-fields label, .lyrics-field { display: flex; flex-direction: column; gap: 6px; color: #777780; font-size: 12px; min-width: 0; }
.metadata-fields input { width: 100%; height: 33px; box-sizing: border-box; border: 1px solid #cfcfd5; border-radius: 5px; background: #fff; color: #33333a; padding: 0 9px; font-size: 12px; user-select: text; }.metadata-fields input:focus, .lyrics-field textarea:focus { outline: 2px solid #d13c8628; border-color: #d13c86; }.id-field { display: flex; gap: 6px; min-width: 0; }.id-field input { flex: 1; min-width: 0; font-family: Consolas, monospace; }.id-field button { flex: 0 0 auto; display: inline-flex; align-items: center; gap: 4px; background: white; color: #bd2c76; border: 1px solid #d4cad2; border-radius: 5px; padding: 0 9px; cursor: pointer; font-size: 11px; }
.lyrics-field { flex: 1; min-height: 0; }.lyrics-field textarea { flex: 1; min-height: 100px; resize: none; padding: 11px; box-sizing: border-box; border: 1px solid #cfcfd5; border-radius: 5px; background: white; color: #44444d; font: 12px/1.65 Consolas, "Microsoft YaHei UI", monospace; white-space: pre; overflow: auto; user-select: text; }
.validation-status { min-height: 21px; display: flex; align-items: center; gap: 5px; margin-top: 8px; font-size: 11px; color: #92929a; }.validation-status.valid { color: #478468; }.validation-status.invalid { color: #c03950; }.validation-status span { overflow-wrap: anywhere; }.editor-note { margin: 3px 0; color: #92929a; font-size: 11px; }
.editor-footer { display: flex; align-items: center; gap: 8px; padding-top: 14px; }.editor-footer p { flex: 1; min-width: 0; margin: 0; color: #bd2c76; font-size: 11px; overflow-wrap: anywhere; }.editor-footer p.error { color: #c03950; }.editor-button { height: 33px; border-radius: 5px; padding: 0 17px; display: inline-flex; align-items: center; gap: 6px; cursor: pointer; font-size: 12px; }.editor-button.secondary { border: 1px solid #cfcfd5; background: white; color: #666670; }.editor-button.primary { border: 1px solid #d13c86; background: #d13c86; color: white; }button:disabled { opacity: .45; cursor: default; }input:disabled, textarea:disabled { opacity: .65; }
@keyframes spin { to { transform: rotate(360deg); } }.spinning { animation: spin 1s linear infinite; }
</style>
