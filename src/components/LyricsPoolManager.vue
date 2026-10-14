<template>
  <div class="pool-workspace">
    <div class="pool-heading">
      <div class="pool-tabs" role="tablist" aria-label="歌词池">
        <span class="pool-tab-slider" :class="{ right: pool === 'manual' }" aria-hidden="true" />
        <button v-for="tab in tabs" :key="tab.id" role="tab" :aria-selected="pool === tab.id" :class="{ active: pool === tab.id }" :disabled="actionBusy" @click="pool = tab.id">{{ tab.label }}</button>
      </div>
      <span class="pool-count">{{ entries.length }} 首歌曲</span>
      <button class="pool-refresh" aria-label="刷新歌词池" title="刷新列表" :disabled="busy" @click="loadEntries"><RefreshCw :size="16" :class="{ spinning: loading }" /></button>
    </div>
    <p class="pool-description">{{ pool === 'cache' ? '按最近使用顺序保存，超出缓存数量的歌曲会自动移除。' : '永久保存手动匹配的歌词，不受缓存数量限制。' }}</p>

    <div class="pool-table-wrap" :aria-busy="loading">
      <table class="pool-table" role="grid" aria-label="歌词池歌曲" aria-multiselectable="false">
        <colgroup><col class="number-column" /><col /><col class="artist-column" /><col /><col class="id-column" /><col class="timing-column" /></colgroup>
        <thead><tr><th scope="col">序号</th><th scope="col">歌名</th><th scope="col">艺人</th><th scope="col">专辑</th><th scope="col">Apple Music ID</th><th scope="col">歌词类型</th></tr></thead>
        <tbody>
          <tr v-for="(entry, index) in entries" :key="entry.id" :class="{ selected: selectedId === entry.id }" :aria-selected="selectedId === entry.id" :tabindex="selectedId === entry.id || (!selectedId && index === 0) ? 0 : -1" @click="selectedId = entry.id" @dblclick="pool === 'manual' && editEntry(entry)" @keydown="onRowKey($event, index)">
            <td class="row-number">{{ index + 1 }}</td><td :title="entry.track.title" class="song-title">{{ entry.track.title }}</td><td :title="entry.track.artist">{{ entry.track.artist || '—' }}</td><td :title="entry.track.album || ''">{{ entry.track.album || '—' }}</td><td class="song-id">{{ entry.appleMusicId || '—' }}</td><td><span class="timing-tag" :class="{ word: entry.timing === 'word' }">{{ entry.timing === 'word' ? '逐字' : '逐行' }}</span></td>
          </tr>
        </tbody>
      </table>
      <div v-if="!entries.length" class="pool-empty"><Database :size="30" /><strong>{{ loading ? '正在读取歌词池…' : error ? '暂时无法读取歌词池' : '这个歌词池还是空的' }}</strong><span>{{ pool === 'cache' ? '播放歌曲并成功获取歌词后，会自动加入缓存池。' : '点击下方“添加”，或将缓存池中的歌曲保存到这里。' }}</span></div>
    </div>

    <div class="pool-toolbar" aria-label="歌词池操作">
      <button class="pool-command" :disabled="pool === 'cache' || busy" title="添加手动匹配歌词" @click="addEntry"><Plus :size="16" />添加</button>
      <button class="pool-command" :disabled="!selected || busy" title="删除选中的歌曲及歌词" @click="deleteEntry"><Trash2 :size="16" />删除</button>
      <button class="pool-command" :disabled="pool !== 'manual' || !selected || busy" :title="pool === 'cache' ? '请先保存到手动匹配池再编辑' : '编辑选中条目，也可以双击歌曲'" @click="editEntry(selected!)"><Pencil :size="16" />编辑</button>
      <button class="pool-command save-command" :disabled="pool !== 'cache' || !selected || busy" title="将选中缓存保存到手动匹配池" @click="promoteEntry"><Save :size="16" />保存到手动匹配池</button>
      <span class="selection-hint">{{ selected ? `已选择第 ${selectedIndex + 1} 项` : pool === 'manual' ? '单击选择，双击编辑' : '先保存到手动匹配池，即可编辑' }}</span>
    </div>
    <p class="pool-feedback" :class="{ error: !!error }" role="status">{{ error || notice }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { ask } from '@tauri-apps/plugin-dialog';
import { Database, Pencil, Plus, RefreshCw, Save, Trash2 } from '@lucide/vue';
import type { LyricsPoolEntry, LyricsPoolKind } from '../types';

const tabs: {id: LyricsPoolKind; label: string}[] = [{id: 'cache', label: '缓存池'}, {id: 'manual', label: '手动匹配池'}];
const pool = ref<LyricsPoolKind>('cache');
const entries = ref<LyricsPoolEntry[]>([]);
const selectedId = ref<string | null>(null);
const selectedIndex = computed(() => entries.value.findIndex(entry => entry.id === selectedId.value));
const selected = computed(() => entries.value[selectedIndex.value] ?? null);
const loading = ref(false), actionBusy = ref(false), error = ref(''), notice = ref('');
const busy = computed(() => loading.value || actionBusy.value);
let generation = 0, disposed = false;
const unlisten: UnlistenFn[] = [];

async function loadEntries() {
  const request = ++generation, target = pool.value;
  loading.value = true; error.value = '';
  try {
    const result = await invoke<LyricsPoolEntry[]>('list_lyrics_pool', {pool: target});
    if (request !== generation || disposed) return;
    entries.value = result;
    if (!result.some(entry => entry.id === selectedId.value)) selectedId.value = null;
  } catch (reason) { if (request === generation && !disposed) error.value = String(reason); }
  finally { if (request === generation && !disposed) loading.value = false; }
}
watch(pool, () => { entries.value = []; selectedId.value = null; notice.value = ''; void loadEntries(); });
onMounted(async () => {
  for (const event of ['lyrics-pools-changed', 'settings-opened']) {
    const stop = await listen(event, () => { void loadEntries(); });
    if (disposed) stop(); else unlisten.push(stop);
  }
  await loadEntries();
});
onUnmounted(() => { disposed = true; generation++; unlisten.forEach(stop => stop()); });

async function runAction(action: () => Promise<void>) {
  if (busy.value) return;
  actionBusy.value = true; error.value = ''; notice.value = '';
  try { await action(); } catch (reason) { error.value = String(reason); }
  finally { actionBusy.value = false; }
}
function addEntry() {
  if (pool.value !== 'manual') return;
  void runAction(() => invoke('open_lyrics_pool_editor', {pool: 'manual', id: null}));
}
function editEntry(entry: LyricsPoolEntry) {
  if (!entry || pool.value !== 'manual') return;
  selectedId.value = entry.id;
  void runAction(() => invoke('open_lyrics_pool_editor', {pool: pool.value, id: entry.id}));
}
async function deleteEntry() {
  const entry = selected.value, target = pool.value;
  if (!entry) return;
  await runAction(async () => {
    if (!await ask(`确定从${target === 'cache' ? '缓存池' : '手动匹配池'}删除“${entry.track.title}”及对应歌词吗？`, {title: '删除歌词', kind: 'warning', okLabel: '删除', cancelLabel: '取消'})) return;
    await invoke('delete_lyrics_pool_entry', {pool: target, id: entry.id, revision: entry.revision});
    selectedId.value = null; notice.value = '已删除歌曲及对应歌词'; await loadEntries();
  });
}
async function promoteEntry() {
  const entry = selected.value;
  if (pool.value !== 'cache' || !entry) return;
  await runAction(async () => {
    await invoke('promote_lyrics_pool_entry', {id: entry.id, revision: entry.revision});
    notice.value = '已保存到手动匹配池'; await loadEntries();
  });
}
async function onRowKey(event: KeyboardEvent, index: number) {
  if (event.key === 'Enter') { event.preventDefault(); editEntry(entries.value[index]); }
  if (event.key === ' ') { event.preventDefault(); selectedId.value = entries.value[index].id; }
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const next = Math.max(0, Math.min(entries.value.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    const row = event.currentTarget as HTMLElement;
    selectedId.value = entries.value[next].id;
    await nextTick(); (row.parentElement?.children[next] as HTMLElement | undefined)?.focus();
  }
}
</script>

<style scoped>
.pool-workspace { min-height: 0; flex: 1; display: flex; flex-direction: column; padding-top: 18px; }
.pool-heading { display: flex; align-items: center; gap: 12px; }
.pool-tabs { position: relative; display: flex; background: #e6e6ec; padding: 3px; border-radius: 8px; width: 260px; flex: 0 0 260px; }
.pool-tabs button { position: relative; width: 50%; height: 32px; border: 0; background: transparent; color: #777780; font-size: 12px; cursor: pointer; transition: color .16s; }
.pool-tabs button.active { color: #bd2c76; font-weight: 650; }
.pool-tab-slider { position: absolute; top: 3px; bottom: 3px; left: 3px; width: calc(50% - 3px); background: white; border-radius: 6px; box-shadow: 0 1px 4px #24242a14; transition: transform .18s ease; }
.pool-tab-slider.right { transform: translateX(100%); }
.pool-count { margin-left: auto; color: #85858e; font-size: 12px; }
.pool-refresh { display: grid; place-items: center; width: 30px; height: 30px; border: 0; background: transparent; color: #777780; border-radius: 5px; cursor: pointer; }
.pool-description { margin: 12px 0 16px; color: #85858e; font-size: 12px; }
.pool-table-wrap { flex: 1; min-height: 130px; overflow: auto; border: 1px solid #dcdce3; background: #fff; border-radius: 7px; position: relative; }
.pool-table { width: 100%; min-width: 680px; table-layout: fixed; border-collapse: separate; border-spacing: 0; text-align: left; font-size: 12px; }
.pool-table .number-column { width: 52px; }.pool-table .artist-column { width: 15%; }.pool-table .id-column { width: 124px; }.pool-table .timing-column { width: 80px; }
.pool-table th { position: sticky; top: 0; z-index: 1; height: 39px; background: #ededf2; color: #74747e; font-size: 11px; font-weight: 500; border-bottom: 1px solid #dcdce3; }
.pool-table th, .pool-table td { padding: 0 12px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pool-table td { height: 46px; border-bottom: 1px solid #ededf2; color: #777780; }
.pool-table tbody tr { cursor: default; outline: none; }.pool-table tbody tr:hover { background: #f7f7fa; }.pool-table tr.selected { background: #fbeaf3 !important; }.pool-table tr.selected td:first-child { box-shadow: inset 3px 0 #d13c86; }.pool-table tr:focus-visible { outline: 2px solid #d13c86; outline-offset: -2px; }
.pool-table .song-title { color: #33333a; font-weight: 600; }.pool-table .row-number { color: #a0a0a8; }.song-id { font: 11px Consolas, monospace; }
.timing-tag { display: inline-block; border-radius: 4px; padding: 3px 7px; background: #ededf2; color: #777780; font-size: 10px; }.timing-tag.word { background: #f8e5f0; color: #bd2c76; }
.pool-empty { position: absolute; inset: 40px 0 0; display: flex; flex-direction: column; justify-content: center; align-items: center; gap: 10px; color: #b1b1bb; }.pool-empty strong { color: #777780; font-size: 13px; font-weight: 500; }.pool-empty span { color: #96969e; font-size: 11px; padding: 0 20px; text-align: center; }
.pool-toolbar { display: flex; align-items: center; gap: 8px; padding-top: 14px; }
.pool-command { height: 32px; display: inline-flex; align-items: center; justify-content: center; gap: 6px; border: 1px solid #ceced4; border-radius: 5px; padding: 0 12px; background: white; color: #55555e; cursor: pointer; font-size: 12px; }.pool-command:not(:disabled):hover { border-color: #d13c86; color: #bd2c76; }.save-command:not(:disabled) { color: #bd2c76; }.pool-command:disabled, .pool-refresh:disabled, .pool-tabs button:disabled { opacity: .4; cursor: default; }
.selection-hint { margin-left: auto; color: #96969e; font-size: 11px; }.pool-feedback { min-height: 18px; margin: 10px 0 0; color: #bd2c76; font-size: 12px; }.pool-feedback.error { color: #c03950; }
@keyframes spin { to { transform: rotate(360deg); } }.spinning { animation: spin 1s linear infinite; }
</style>
