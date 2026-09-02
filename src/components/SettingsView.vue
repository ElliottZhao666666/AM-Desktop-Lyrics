<template>
  <main class="settings-shell" @contextmenu="handleContextMenu">
    <aside class="settings-sidebar">
      <div class="brand-mark"><img :src="appIcon" alt="AMDL" /><span>AMDL</span></div>
      <nav aria-label="设置分类">
        <button v-for="item in sections" :key="item.id" class="nav-item" :class="{ active: activeSection === item.id }" @click="activeSection = item.id">
          <component :is="item.icon" :size="17" /><span>{{ item.label }}</span>
        </button>
      </nav>
    </aside>

    <section class="settings-content">
      <header class="settings-header"><div><p class="eyebrow">AMDL</p><h1>{{ currentSection.label }}</h1></div></header>

      <div v-if="activeSection === 'general'" class="settings-list">
        <div class="setting-row"><div><strong>歌词时间偏移 (ms)</strong><p>调整后即时生效；点击“应用”后写入配置，并在下次运行时读取。</p></div><input :value="lyricsOffsetInput" class="offset-input" type="text" inputmode="numeric" @input="updateLyricsOffsetInput" /></div>
        <div class="setting-row"><div><strong>自动转换繁体歌词到简体</strong><p>Apple Music 缓存命中时自动转换华语歌词。</p></div><button class="switch" :class="{ on: settings.autoConvertTraditionalToSimplified }" role="switch" :aria-checked="settings.autoConvertTraditionalToSimplified" @click="settings.autoConvertTraditionalToSimplified = !settings.autoConvertTraditionalToSimplified"><span /></button></div>
        <div class="setting-row"><div><strong>记忆歌词窗口位置和大小</strong><p>保存拖动和缩放后的歌词窗口状态。</p></div><button class="switch" :class="{ on: settings.rememberLyricsWindowBounds }" role="switch" :aria-checked="settings.rememberLyricsWindowBounds" @click="settings.rememberLyricsWindowBounds = !settings.rememberLyricsWindowBounds"><span /></button></div>
        <div class="setting-row"><div><strong>歌词缓存歌曲数</strong><p>保存最近播放的歌词，重复播放时优先读取。</p></div><input v-model.number="settings.localCacheLimit" class="number-input" type="number" min="1" max="50" step="1" /></div>
      </div>

      <div v-else-if="activeSection === 'appearance'" class="appearance-workspace">
        <div class="appearance-toolbar">
          <label>预设主题<select v-model="selectedPreset" @change="applyPreset"><option value="custom">自定义</option><option v-for="preset in PRESETS" :key="preset.id" :value="preset.id">{{ preset.name }}</option></select></label>
          <label>主题名称<input v-model="settings.themeName" class="theme-name" /></label>
          <div class="file-actions"><button class="icon-command" title="加载 .amdlcss" @click="importTheme"><FolderOpen :size="16" />加载</button><button class="icon-command" title="保存为 .amdlcss" @click="exportTheme"><Save :size="16" />导出</button></div>
        </div>

        <div class="appearance-grid">
          <div class="editor-pane">
            <section class="editor-section">
              <h2>整体文字</h2>
              <div class="control-grid">
                <label class="wide">字体<select v-model="editor.fontFamily" @change="syncCss"><option v-for="font in systemFonts" :key="font" :value="font">{{ font }}</option><option value="custom">自定义字体…</option></select></label>
                <label v-if="editor.fontFamily === 'custom'" class="wide">自定义字体<input v-model="customFontFamily" placeholder="例如：霞鹜文楷" @input="syncCustomFont" /></label>
                <label>字号<input v-model.number="editor.fontSize" type="number" min="12" max="72" @input="syncCss" /></label>
                <label>字重<select v-model.number="editor.fontWeight" @change="syncCss"><option :value="400">常规</option><option :value="600">半粗</option><option :value="700">粗体</option><option :value="800">特粗</option><option :value="900">黑体</option></select></label>
                <label>对齐<select v-model="editor.align" @change="syncCss"><option value="left">左对齐</option><option value="center">居中</option><option value="right">右对齐</option></select></label>
                <label>透明度<input v-model.number="editor.opacity" type="range" min="10" max="100" @input="syncCss" /><span class="range-value">{{ editor.opacity }}%</span></label>
                <label class="check-label"><input v-model="editor.italic" type="checkbox" @change="syncCss" />倾斜</label>
              </div>
            </section>

            <TextStyleEditor title="普通歌词" :paint="editor.normal" @change="syncCss" />
            <TextStyleEditor title="高亮歌词" :paint="editor.active" @change="syncCss" />

            <section class="editor-section">
              <h2>进度条</h2>
              <div class="control-grid paint-controls">
                <label>填充<select v-model="editor.progress.mode" @change="syncCss"><option value="solid">纯色</option><option value="gradient">线性渐变</option></select></label>
                <label>颜色<input v-model="editor.progress.color" type="color" @input="syncCss" /></label>
                <label v-if="editor.progress.mode === 'gradient'">结束色<input v-model="editor.progress.colorEnd" type="color" @input="syncCss" /></label>
                <label v-if="editor.progress.mode === 'gradient'">角度<input v-model.number="editor.progress.angle" type="number" min="0" max="360" @input="syncCss" /></label>
              </div>
            </section>
          </div>

          <div class="preview-pane">
            <div class="preview-title">预览</div>
            <div class="theme-preview">
              <div class="amdl-lyrics"><div class="amdl-lyrics-stack"><div class="amdl-lyric-line">昨日的风停在窗外</div><div class="amdl-lyric-line is-active">此刻听见你在 Say Hi</div></div></div>
              <div class="preview-progress"><div class="amdl-progress-fill" style="width: 62%" /></div>
            </div>
            <div class="source-header"><span>CSS 源码</span><button class="text-command" @click="resetDefault">恢复默认</button></div>
            <textarea v-model="settings.themeCss" class="css-editor" spellcheck="false" @input="selectedPreset = 'custom'" />
          </div>
        </div>
        <component :is="'style'">{{ settings.themeCss }}</component>
      </div>

      <div v-else-if="activeSection === 'logs'" class="logs-workspace">
        <div class="logs-toolbar">
          <button class="icon-command" title="重新读取当前日志" @click="refreshLog"><RefreshCw :size="16" />刷新日志</button>
          <button class="icon-command" title="在资源管理器中打开日志目录" @click="openLogDirectory"><FolderOpen :size="16" />打开日志目录</button>
        </div>
        <textarea :value="logText" class="log-viewer" readonly spellcheck="false" aria-label="当前运行日志" />
      </div>

      <div v-else-if="activeSection === 'lyricsMatch'" class="match-workspace">
        <div class="now-playing-strip">
          <div class="track-summary">
            <strong>{{ currentTrack.title || '等待播放...' }}</strong>
            <span>{{ currentTrack.artist || '暂无歌手信息' }}</span>
            <small>{{ currentTrack.album || '暂无专辑信息' }}</small>
          </div>
          <div class="match-quick-actions">
            <button class="icon-command" :disabled="!canMatch || matchBusy" @click="rescanLocalLyrics"><Database :size="16" />重新扫描本地缓存</button>
            <button class="icon-command" :disabled="!canMatch || matchBusy" @click="refetchQqLyrics"><CloudDownload :size="16" />重新获取 Q音歌词</button>
          </div>
        </div>

        <form class="match-search-form" @submit.prevent="searchLyrics">
          <label>歌名<input v-model="matchQuery.title" autocomplete="off" /></label>
          <label>歌手<input v-model="matchQuery.artist" autocomplete="off" /></label>
          <label>专辑<input v-model="matchQuery.album" autocomplete="off" /></label>
          <button class="button primary search-button" type="submit" :disabled="matchBusy || !matchQuery.title.trim()"><Search :size="16" />搜索</button>
        </form>

        <div class="match-grid">
          <section class="match-results" aria-label="Q音搜索结果">
            <div class="pane-heading"><span>搜索结果</span><small>{{ matchCandidates.length }} 首</small></div>
            <div v-if="matchBusy && !matchCandidates.length" class="match-placeholder">正在请求 Q音...</div>
            <div v-else-if="!matchCandidates.length" class="match-placeholder">输入曲目信息后开始搜索</div>
            <button
              v-for="candidate in matchCandidates"
              :key="candidate.songMid"
              type="button"
              class="candidate-row"
              :class="{ selected: selectedCandidate?.songMid === candidate.songMid }"
              @click="selectCandidate(candidate)"
            >
              <span class="candidate-title">{{ candidate.track.title }}</span>
              <span>{{ candidate.track.artist || '未知歌手' }}</span>
              <span>{{ candidate.track.album || '未知专辑' }}</span>
              <time>{{ formatDuration(candidate.durationMs) }}</time>
            </button>
          </section>

          <section class="lyrics-preview-pane">
            <div class="pane-heading"><span>歌词全文预览</span><small>{{ selectedCandidate?.track.title || '未选择' }}</small></div>
            <textarea :value="previewText" class="lyrics-preview" readonly spellcheck="false" aria-label="选中歌曲歌词全文" />
            <div class="apply-lyrics-actions"><button class="button secondary apply-lyrics-button" :disabled="!selectedCandidate || !previewText || matchBusy || !canMatch" @click="applySelectedLyrics"><Check :size="16" />应用此歌词</button><button class="button primary apply-lyrics-button" :disabled="!selectedCandidate || !previewText || matchBusy || !canMatch" @click="applyAndAlwaysUseSelectedLyrics"><Check :size="16" />应用并始终使用此歌词</button></div>
          </section>
        </div>
      </div>

      <div v-else-if="activeSection === 'lyricsSources'" class="sources-workspace">
        <div class="sources-intro"><strong>歌词获取渠道</strong><p>按优先级依次尝试启用的渠道。手动匹配结果和永久匹配始终优先。</p></div>
        <div class="source-list">
          <div v-for="source in sortedLyricsSources" :key="source.id" class="source-card" :class="{ disabled: source.id === 'neteaseMusic' }">
            <div class="source-order">{{ source.priority }}</div>
            <div class="source-info"><strong>{{ source.name }}</strong><p>{{ source.id === 'appleMusicApi' ? '需要 Apple Music API Token' : source.id === 'appleMusicCache' ? '读取 Apple Music Windows 版本地缓存' : source.id === 'qqMusic' ? '在线搜索 QQ 音乐歌词' : source.id === 'neteaseMusic' ? '暂未开放' : '使用匹配歌词页面中设置为始终使用的歌词' }}</p></div>
            <button v-if="source.id === 'appleMusicApi'" class="text-command" @click="tokensExpanded = !tokensExpanded">{{ tokensExpanded ? '收起 Token' : '配置 Token' }}</button>
            <button class="switch" :class="{ on: source.enabled }" :disabled="source.id === 'neteaseMusic'" role="switch" :aria-checked="source.enabled" @click="source.enabled = !source.enabled"><span /></button>
          </div>
        </div>
        <div v-if="tokensExpanded" class="token-panel">
          <label>Authorization Token<input v-model="config.amApiTokens.authToken" type="password" autocomplete="off" /></label>
          <label>Media User Token<input v-model="config.amApiTokens.mediaUserToken" type="password" autocomplete="off" /></label>
          <button class="text-command" @click="openExternal('https://amdl.ybjun.com/help/get-token')">帮助获取 Token</button>
          <p>Token 仅保存在本机配置中，点击底部“保存并生效”后才会写入。</p>
        </div>
      </div>

      <div v-else class="about-panel">
        <div class="about-content">
          <!-- 核心标志与版本信息 -->
          <img class="about-logo" :src="appIcon" alt="AMDL 软件图标" />
          <h1 class="about-app-name">Apple Music Desktop Lyrics</h1>
          <p class="about-version">版本 {{ appVersion }}</p>

          <div class="about-divider"></div>

          <!-- 版权与作者信息 -->
          <p class="about-copyright">
            Designed & Developed by 
            <a href="https://www.ybjun.com" @click.prevent="openExternal('https://www.ybjun.com')">月半菌</a>
          </p>
          
          <p class="about-license">© 2026 月半菌. All rights reserved.</p>
          <p class="about-license">本软件遵循 MIT 开源协议发布，详见 <a href="https://opensource.org/licenses/MIT" @click.prevent="openExternal('https://opensource.org/licenses/MIT')">LICENSE</a>。</p>
          <!-- 外部链接与操作区 -->
          <div class="about-links">
            <button class="link-button" @click.prevent="openExternal('https://amdl.ybjun.com')">
              <Globe :size="15" /> 官方网站
            </button>
            <button class="link-button" @click.prevent="openExternal('https://github.com/ElliottZhao666666/AM-Desktop-Lyrics')">
              <GitBranch :size="15" /> GitHub 仓库
            </button>
            <button class="link-button" type="button" :disabled="isCheckingUpdate" @click="checkForUpdates">
              <RefreshCw :size="15" :class="{ spin: isCheckingUpdate }" /> 检查更新
            </button>
          </div>
        </div>
      </div>

      <footer class="settings-footer"><span class="save-message">{{ message }}</span><div v-if="activeSection === 'general' || activeSection === 'appearance' || activeSection === 'lyricsSources'" class="footer-actions"><button class="button secondary" @click="closeWindow">取消</button><button class="button primary" @click="saveAll">{{ activeSection === 'lyricsSources' ? '保存并生效' : '应用' }}</button></div></footer>
    </section>
  </main>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { message as showMessage, open, save } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ask } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { relaunch } from '@tauri-apps/plugin-process';
import { check } from '@tauri-apps/plugin-updater';
import { Check, CloudDownload, Database, FileText, FolderOpen, Info, Palette, RefreshCw, Save, Search, Settings2, Globe, GitBranch } from '@lucide/vue';
import type { AppConfig, AppSettings, MediaSnapshotPayload, MediaStatePayload, QqLyricsPreview, QqSongCandidate, RuntimeLyricsTimeOffset, ThemeFile, TrackIdentity } from '../types';
import { buildThemeCss, cloneEditor, DEFAULT_EDITOR, PRESETS, type ThemeEditor } from '../theme';
import TextStyleEditor from './TextStyleEditor.vue';
import appIcon from '../../app-icon.png';

const appVersion = __APP_VERSION__;

const sections = [{ id: 'general', label: '通用', icon: Settings2 }, { id: 'appearance', label: '外观', icon: Palette }, { id: 'lyricsMatch', label: '匹配歌词', icon: Search }, { id: 'lyricsSources', label: '歌词渠道', icon: CloudDownload }, { id: 'logs', label: '日志', icon: FileText }, { id: 'about', label: '关于', icon: Info }] as const;
const activeSection = ref<typeof sections[number]['id']>('general');
const settings = ref<AppSettings>({
  autoConvertTraditionalToSimplified: true,
  localCacheLimit: 10,
  themeName: 'AMDL 默认',
  themeCss: buildThemeCss(DEFAULT_EDITOR),
  lyricsTimeOffsetMs: 0,
  rememberLyricsWindowBounds: false,
  lyricsWindowBounds: null,
});
const editor = ref<ThemeEditor>(cloneEditor(DEFAULT_EDITOR));
const selectedPreset = ref('default');
const customFontFamily = ref('');
const systemFonts = ref<string[]>(['Microsoft YaHei UI']);
const message = ref('');
const logText = ref('');
const currentTrack = ref<TrackIdentity>({ title: '等待播放...', artist: '', album: null });
const matchQuery = ref({ title: '', artist: '', album: '' });
const matchCandidates = ref<QqSongCandidate[]>([]);
const selectedCandidate = ref<QqSongCandidate | null>(null);
const previewText = ref('');
const matchBusy = ref(false);
const isCheckingUpdate = ref(false);
const currentSection = computed(() => sections.find((item) => item.id === activeSection.value) ?? sections[0]);
const canMatch = computed(() => Boolean(currentTrack.value.title.trim() && currentTrack.value.artist.trim() && currentTrack.value.title !== '等待播放...'));
let unlistenLyricsTimeOffset: UnlistenFn | null = null;
let unlistenMediaState: UnlistenFn | null = null;
let lyricsTimeOffsetRevision = -1;
let latestLyricsTimeOffset: RuntimeLyricsTimeOffset = { offsetMs: 0, revision: -1 };
let pendingLyricsOffsetUpdate: Promise<void> = Promise.resolve();
const lyricsOffsetInput = ref('0');
const config = ref<AppConfig>({ lyricsSources: [], amApiTokens: { authToken: '', mediaUserToken: '' }, autoChs: true });
const tokensExpanded = ref(false);
const sortedLyricsSources = computed(() => [...config.value.lyricsSources].sort((a, b) => a.priority - b.priority));

watch(activeSection, (section) => {
  if (section === 'logs') void refreshLog();
});

function applyLyricsTimeOffset(payload: RuntimeLyricsTimeOffset) {
  if (payload.revision < lyricsTimeOffsetRevision) return;
  lyricsTimeOffsetRevision = payload.revision;
  latestLyricsTimeOffset = payload;
  settings.value.lyricsTimeOffsetMs = payload.offsetMs;
  lyricsOffsetInput.value = String(payload.offsetMs);
}

onMounted(async () => {
  try {
    unlistenLyricsTimeOffset = await listen<RuntimeLyricsTimeOffset>('lyrics-time-offset-changed', (event) => {
      applyLyricsTimeOffset(event.payload);
    });
    unlistenMediaState = await listen<MediaStatePayload>('media-state-changed', (event) => updateCurrentTrack(event.payload.track));
    const [loadedSettings, loadedConfig, runtimeOffset, fonts, mediaSnapshot] = await Promise.all([
      invoke<AppSettings>('get_settings'),
      invoke<AppConfig>('get_config'),
      invoke<RuntimeLyricsTimeOffset>('get_runtime_lyrics_time_offset'),
      invoke<string[]>('get_system_fonts'),
      invoke<MediaSnapshotPayload>('get_media_snapshot'),
    ]);
    settings.value = loadedSettings;
    config.value = loadedConfig;
    applyLyricsTimeOffset(runtimeOffset);
    applyLyricsTimeOffset(latestLyricsTimeOffset);
    systemFonts.value = fonts.length > 0 ? fonts : systemFonts.value;
    updateCurrentTrack(mediaSnapshot.media.track);
    if (!systemFonts.value.includes(editor.value.fontFamily)) systemFonts.value.unshift(editor.value.fontFamily);
  } catch (error) { message.value = String(error); }
});

onUnmounted(() => { unlistenLyricsTimeOffset?.(); unlistenMediaState?.(); });

function updateCurrentTrack(track: TrackIdentity) {
  const changed = track.title !== currentTrack.value.title || track.artist !== currentTrack.value.artist || track.album !== currentTrack.value.album;
  currentTrack.value = track;
  if (!changed) return;
  matchQuery.value = { title: track.title === '等待播放...' ? '' : track.title, artist: track.artist, album: track.album ?? '' };
  matchCandidates.value = [];
  selectedCandidate.value = null;
  previewText.value = '';
}

function queryTrack(): TrackIdentity {
  return { title: matchQuery.value.title.trim(), artist: matchQuery.value.artist.trim(), album: matchQuery.value.album.trim() || null };
}

function formatDuration(durationMs: number | null) {
  if (durationMs === null) return '--:--';
  const seconds = Math.round(durationMs / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

async function rescanLocalLyrics() {
  matchBusy.value = true;
  try {
    const found = await invoke<boolean>('rescan_local_lyrics', { track: currentTrack.value });
    await showMessage(found ? '本地缓存已刷新' : '本地缓存中没有找到当前歌曲歌词', { title: 'AMDL', kind: 'info' });
  } catch (error) { await showMessage(String(error), { title: '扫描失败', kind: 'error' }); }
  finally { matchBusy.value = false; }
}

async function refetchQqLyrics() {
  matchBusy.value = true;
  try {
    const found = await invoke<boolean>('refetch_qq_lyrics', { track: currentTrack.value });
    await showMessage(found ? 'Q音歌词已刷新' : 'Q音没有找到匹配歌词', { title: 'AMDL', kind: 'info' });
  } catch (error) { await showMessage(String(error), { title: '获取失败', kind: 'error' }); }
  finally { matchBusy.value = false; }
}

async function searchLyrics() {
  matchBusy.value = true;
  matchCandidates.value = [];
  selectedCandidate.value = null;
  previewText.value = '';
  try {
    matchCandidates.value = await invoke<QqSongCandidate[]>('search_qq_lyrics', { track: queryTrack() });
    if (!matchCandidates.value.length) await showMessage('没有找到候选歌曲', { title: 'AMDL', kind: 'info' });
  } catch (error) { await showMessage(String(error), { title: '搜索失败', kind: 'error' }); }
  finally { matchBusy.value = false; }
}

async function selectCandidate(candidate: QqSongCandidate) {
  selectedCandidate.value = candidate;
  previewText.value = '';
  matchBusy.value = true;
  try {
    const preview = await invoke<QqLyricsPreview>('preview_qq_lyrics', { candidate });
    if (selectedCandidate.value?.songMid === candidate.songMid) previewText.value = preview.lyricsText;
  } catch (error) { await showMessage(String(error), { title: '预览失败', kind: 'error' }); }
  finally { matchBusy.value = false; }
}

async function applySelectedLyrics() {
  if (!selectedCandidate.value) return;
  matchBusy.value = true;
  try {
    await invoke('apply_manual_lyrics', { track: currentTrack.value, candidate: selectedCandidate.value });
    await showMessage('歌词已应用并覆盖普通缓存', { title: 'AMDL', kind: 'info' });
  } catch (error) { await showMessage(String(error), { title: '应用失败', kind: 'error' }); }
  finally { matchBusy.value = false; }
}

async function applyAndAlwaysUseSelectedLyrics() {
  if (!selectedCandidate.value) return;
  matchBusy.value = true;
  try { await invoke('apply_custom_lyrics', { track: currentTrack.value, candidate: selectedCandidate.value }); await showMessage('歌词已应用并保存为永久匹配', { title: 'AMDL', kind: 'info' }); }
  catch (error) { await showMessage(String(error), { title: '应用失败', kind: 'error' }); }
  finally { matchBusy.value = false; }
}

function syncCss() { settings.value.themeCss = buildThemeCss(editor.value); selectedPreset.value = 'custom'; }
function syncCustomFont() { editor.value.fontFamily = customFontFamily.value || 'Microsoft YaHei UI'; syncCss(); }
function applyPreset() { const preset = PRESETS.find((item) => item.id === selectedPreset.value); if (!preset) return; editor.value = cloneEditor(preset.editor); settings.value.themeName = preset.name; settings.value.themeCss = buildThemeCss(editor.value); }
function resetDefault() { selectedPreset.value = 'default'; applyPreset(); }

function applyRuntimeLyricsOffset() {
  const rawValue: unknown = settings.value.lyricsTimeOffsetMs;
  if (rawValue === '') return;
  const value = Number(rawValue);
  if (!Number.isFinite(value)) return;
  settings.value.lyricsTimeOffsetMs = Math.trunc(value);
  const offsetMs = settings.value.lyricsTimeOffsetMs;
  pendingLyricsOffsetUpdate = pendingLyricsOffsetUpdate.then(async () => {
    try {
      applyLyricsTimeOffset(await invoke<RuntimeLyricsTimeOffset>('set_runtime_lyrics_time_offset', { offsetMs }));
    } catch (error) {
      message.value = String(error);
    }
  });
}

function updateLyricsOffsetInput(event: Event) {
  const value = (event.target as HTMLInputElement).value;
  lyricsOffsetInput.value = value;
  if (value === '' || value === '-') return;
  if (!Number.isFinite(Number(value))) return;
  settings.value.lyricsTimeOffsetMs = Math.trunc(Number(value));
  applyRuntimeLyricsOffset();
}

function handleContextMenu(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (target?.closest('input, textarea, [contenteditable="true"]')) return;
  event.preventDefault();
}

async function importTheme() {
  const path = await open({ multiple: false, filters: [{ name: 'AMDL 主题', extensions: ['amdlcss'] }] });
  if (!path) return;
  try { const theme = await invoke<ThemeFile>('import_theme_file', { path }); settings.value.themeName = theme.name; settings.value.themeCss = theme.css; selectedPreset.value = 'custom'; message.value = `已加载 ${theme.name}`; } catch (error) { message.value = String(error); }
}
async function exportTheme() {
  const path = await save({ defaultPath: `${settings.value.themeName || 'AMDL主题'}.amdlcss`, filters: [{ name: 'AMDL 主题', extensions: ['amdlcss'] }] });
  if (!path) return;
  try { await invoke('export_theme_file', { path, name: settings.value.themeName, css: settings.value.themeCss }); message.value = '主题已导出'; } catch (error) { message.value = String(error); }
}
async function refreshLog() {
  try {
    logText.value = await invoke<string>('read_current_log');
    message.value = '';
  } catch (error) {
    message.value = String(error);
  }
}
async function openLogDirectory() {
  try {
    await invoke('open_log_dir');
    message.value = '';
  } catch (error) {
    message.value = String(error);
  }
}
async function saveSettings() {
  settings.value.localCacheLimit = Math.max(1, Math.min(50, Math.round(settings.value.localCacheLimit || 10)));
  const rawLyricsTimeOffset: unknown = settings.value.lyricsTimeOffsetMs;
  if (rawLyricsTimeOffset === '' || !Number.isFinite(Number(rawLyricsTimeOffset))) {
    settings.value.lyricsTimeOffsetMs = latestLyricsTimeOffset.offsetMs;
  }
  await pendingLyricsOffsetUpdate;
  settings.value.lyricsTimeOffsetMs = latestLyricsTimeOffset.offsetMs;
  try { await invoke('save_settings', { settings: settings.value }); message.value = '已应用'; setTimeout(() => { message.value = ''; }, 1800); } catch (error) { message.value = String(error); }
}
async function saveAll() {
  if (activeSection.value === 'lyricsSources') {
    try { await invoke('save_config', { config: config.value }); message.value = '配置已保存并生效'; } catch (error) { message.value = String(error); }
    return;
  }
  await saveSettings();
}
function closeWindow() { getCurrentWindow().hide(); }
async function openExternal(url: string) {
  try { await openUrl(url); } catch (error) { message.value = String(error); }
}

async function checkForUpdates() {
  if (isCheckingUpdate.value) return;
  isCheckingUpdate.value = true;
  try {
    const update = await check();
    if (!update) {
      await showMessage('当前已是最新版本', { title: 'AMDL', kind: 'info' });
      return;
    }
    const releaseNotes = update.body?.trim() || '暂无更新日志';
    const confirmed = await ask(
      `发现新版本 ${update.version}\n\n${releaseNotes}\n\n是否立即下载并安装？`,
      { title: 'AMDL 更新', kind: 'info' },
    );
    if (!confirmed) return;
    message.value = '正在下载并安装更新…';
    await update.downloadAndInstall();
    await relaunch();
  } catch (error) {
    await showMessage(`检查更新失败：${String(error)}`, { title: 'AMDL', kind: 'error' });
  } finally {
    isCheckingUpdate.value = false;
  }
}
</script>

<style scoped>
.settings-shell { width: 100vw; height: 100vh; display: flex; overflow: hidden; color: #24242a; background: #f4f4f6; font-family: "Segoe UI", "Microsoft YaHei", sans-serif; }
.settings-sidebar { width: 176px; flex: 0 0 176px; padding: 28px 14px; box-sizing: border-box; background: #e9e9ed; border-right: 1px solid #d8d8dd; }.brand-mark { display: flex; align-items: center; gap: 9px; margin: 0 14px 30px; font-size: 18px; font-weight: 750; color: #d9428b; }.brand-mark img { width: 28px; height: 28px; border-radius: 7px; }.nav-item { width: 100%; height: 42px; display: flex; align-items: center; gap: 11px; border: 0; border-radius: 7px; padding: 0 13px; color: #55555e; background: transparent; font-size: 14px; cursor: pointer; }.nav-item:hover { background: #dedee4; }.nav-item.active { color: #bd2c76; background: #fff; box-shadow: 0 1px 3px #00000012; font-weight: 650; }
.settings-content { min-width: 0; flex: 1; padding: 28px 34px 22px; box-sizing: border-box; display: flex; flex-direction: column; }.settings-header { padding-bottom: 18px; border-bottom: 1px solid #dedee3; }.eyebrow { margin: 0 0 5px; color: #c13a7c; font-size: 11px; font-weight: 700; }.settings-header h1 { margin: 0; font-size: 22px; }.settings-list { padding-top: 8px; }.setting-row { min-height: 76px; display: flex; align-items: center; justify-content: space-between; gap: 20px; border-bottom: 1px solid #e1e1e5; }.setting-row strong { font-size: 14px; }.setting-row p { margin: 6px 0 0; color: #85858e; font-size: 12px; }.switch { width: 38px; height: 22px; padding: 2px; border: 0; border-radius: 12px; background: #c6c6cc; cursor: pointer; }.switch span { display: block; width: 18px; height: 18px; border-radius: 50%; background: white; transition: transform .18s; }.switch.on { background: #d53e8b; }.switch.on span { transform: translateX(16px); }
.sources-workspace { flex: 1; min-height: 0; overflow-y: auto; padding-top: 14px; }.sources-intro strong { font-size: 15px; }.sources-intro p, .token-panel p { margin: 6px 0 14px; color: #85858e; font-size: 12px; }.source-list { display: grid; gap: 8px; }.source-card { min-height: 62px; display: flex; align-items: center; gap: 13px; padding: 0 14px; background: #fff; border: 1px solid #e0e0e5; border-radius: 8px; }.source-card.disabled { opacity: .55; }.source-order { width: 25px; height: 25px; display: grid; place-items: center; border-radius: 50%; color: #bd2c76; background: #f9e5f0; font-size: 12px; font-weight: 700; }.source-info { flex: 1; min-width: 0; }.source-info strong { font-size: 13px; }.source-info p { margin: 4px 0 0; color: #85858e; font-size: 11px; }.source-card .text-command { margin-right: 8px; }.token-panel { display: grid; gap: 10px; margin-top: 14px; padding: 16px; background: #fff; border: 1px solid #dedee3; border-radius: 8px; }.token-panel label { display: grid; gap: 5px; color: #777780; font-size: 11px; }.token-panel input { height: 32px; box-sizing: border-box; padding: 0 8px; border: 1px solid #cfcfd5; border-radius: 4px; }.token-panel .text-command { justify-self: start; padding: 0; }.token-panel p { margin: 0; }
/* --- 经典“关于”页面 (Standard About Interface) --- */
.about-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 20px;
  overflow-y: auto;
}

.about-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 100%;
  max-width: 460px;
  text-align: center;
}

.about-logo {
  width: 96px;
  height: 96px;
  border-radius: 22px;
  box-shadow: 0 10px 28px rgba(189, 44, 118, 0.22);
  margin-bottom: 22px;
  user-select: none;
  -webkit-user-drag: none;
}

.about-app-name {
  font-size: 24px;
  font-weight: 700;
  color: #24242a;
  margin: 0 0 6px 0;
  letter-spacing: -0.3px;
}

.about-version {
  font-size: 13px;
  color: #85858e;
  margin: 0 0 28px 0;
  font-family: Consolas, "Cascadia Mono", monospace;
}

/* 优雅的视觉分割线 */
.about-divider {
  width: 48px;
  height: 3px;
  background: #dedee3;
  margin: 0 0 28px 0;
  border-radius: 3px;
}

.about-copyright {
  font-size: 13px;
  color: #55555e;
  margin: 0 0 18px 0;
}

.about-copyright a {
  color: #bd2c76;
  text-decoration: none;
  font-weight: 600;
  transition: color 0.2s;
}

.about-copyright a:hover {
  color: #d13c86;
  text-decoration: underline;
}

/* 居中的按钮式链接 */
.about-links {
  display: flex;
  gap: 12px;
  margin-bottom: 36px;
}

.about-license {
  font-size: 12px;
  color: #777780;
  margin: 0 0 6px 0;
}

.link-button {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 16px;
  background: #fff;
  border: 1px solid #d8d8dd;
  border-radius: 8px;
  color: #414149;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s ease;
  box-shadow: 0 2px 6px rgba(36, 36, 42, 0.03);
}

.link-button:hover {
  background: #fafafc;
  border-color: #cfcfd5;
  box-shadow: 0 4px 10px rgba(36, 36, 42, 0.06);
  transform: translateY(-1px);
}

/* 微信公众号横幅专门适配 */
.about-wechat-banner {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  margin-bottom: 40px;
  padding: 16px 20px;
  background: #fff;
  border: 1px solid #dedee3;
  border-radius: 12px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.02);
  width: 100%;
  box-sizing: border-box;
}

.about-wechat-banner p {
  margin: 0;
  font-size: 12px;
  color: #777780;
  letter-spacing: 0.5px;
}

.about-wechat-banner img {
  width: 100%;
  height: auto;
  border-radius: 6px;
  display: block;
  /* 确保 1280x468 的图像清晰且不越界 */
  object-fit: contain; 
}

/* 底部操作按钮 */
.update-button {
  padding: 0 48px;
  height: 34px;
  font-size: 13px;
}
@keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
.spin { animation: spin 1s linear infinite; }
.appearance-workspace { min-height: 0; flex: 1; display: flex; flex-direction: column; }.appearance-toolbar { display: flex; align-items: end; gap: 14px; padding: 14px 0; border-bottom: 1px solid #dedee3; }.appearance-toolbar label { display: grid; gap: 5px; font-size: 11px; color: #777780; }.appearance-toolbar select { width: 150px; }.theme-name { width: 180px; }.file-actions { margin-left: auto; display: flex; gap: 6px; }.icon-command { height: 31px; display: flex; align-items: center; gap: 6px; border: 1px solid #ceced4; border-radius: 5px; padding: 0 10px; background: #fff; color: #55555e; cursor: pointer; }
.appearance-grid { min-height: 0; flex: 1; display: grid; grid-template-columns: minmax(470px, 1.08fr) minmax(360px, .92fr); gap: 24px; padding-top: 14px; }.editor-pane { min-height: 0; overflow-y: auto; padding: 0 10px 8px 0; }.editor-section { padding: 0 0 16px; margin-bottom: 16px; border-bottom: 1px solid #dfdfe4; }.editor-section h2 { margin: 0 0 11px; font-size: 13px; color: #4d4d55; }.control-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px 12px; }.control-grid label { min-width: 0; display: grid; gap: 5px; color: #7b7b84; font-size: 11px; }.control-grid label.wide { grid-column: span 2; }.control-grid input:not([type="checkbox"]):not([type="range"]), .control-grid select, .appearance-toolbar input, .appearance-toolbar select { box-sizing: border-box; width: 100%; height: 31px; border: 1px solid #cfcfd5; border-radius: 4px; padding: 0 7px; background: white; color: #33333a; }.control-grid input[type="color"] { padding: 3px; }.control-grid input[type="range"] { width: 100%; }.check-label { display: flex !important; align-items: center; gap: 7px !important; min-height: 31px; color: #55555e !important; }.check-label input { accent-color: #d13c86; }.range-value { color: #55555e; }.paint-controls { grid-template-columns: repeat(3, minmax(0, 1fr)); }
.preview-pane { position: sticky; top: 0; align-self: start; min-height: 0; display: flex; flex-direction: column; }.preview-title, .source-header { height: 28px; display: flex; align-items: center; justify-content: space-between; color: #686871; font-size: 12px; font-weight: 650; }.theme-preview { position: relative; height: 176px; display: flex; align-items: center; overflow: hidden; background: #15151a; border: 1px solid #2e2e35; }.theme-preview::before { content: ''; position: absolute; inset: 0; background: linear-gradient(135deg, #23232b, #0d0d11 60%, #2b1823); }.theme-preview .amdl-lyrics { position: relative; width: 100%; padding: 20px; box-sizing: border-box; }.preview-progress { position: absolute; right: 0; bottom: 0; left: 0; height: 3px; background: #ffffff20; }.preview-progress > div { height: 100%; }.source-header { margin-top: 10px; }.text-command { border: 0; background: transparent; color: #bd2c76; cursor: pointer; }.css-editor { min-height: 110px; flex: 1; resize: none; border: 1px solid #cfcfd5; border-radius: 5px; padding: 10px; background: #fff; color: #33333a; font: 11px/1.5 Consolas, monospace; user-select: text; }
.logs-workspace { min-height: 0; flex: 1; display: flex; flex-direction: column; padding-top: 14px; }.logs-toolbar { display: flex; gap: 8px; padding-bottom: 12px; }.log-viewer { min-height: 0; flex: 1; resize: none; box-sizing: border-box; width: 100%; border: 1px solid #cfcfd5; border-radius: 5px; padding: 12px; background: #fff; color: #33333a; font: 12px/1.55 Consolas, "Cascadia Mono", monospace; user-select: text; white-space: pre; }
.match-workspace { min-height: 0; flex: 1; display: flex; flex-direction: column; padding-top: 14px; }.now-playing-strip { min-height: 62px; display: flex; align-items: center; justify-content: space-between; gap: 20px; padding-bottom: 12px; border-bottom: 1px solid #dedee3; }.track-summary { min-width: 0; display: grid; gap: 3px; }.track-summary strong, .track-summary span, .track-summary small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.track-summary strong { font-size: 15px; color: #33333a; }.track-summary span { font-size: 12px; color: #676770; }.track-summary small { font-size: 11px; color: #92929a; }.match-quick-actions { display: flex; flex: 0 0 auto; gap: 7px; }.icon-command:disabled, .button:disabled { opacity: .45; cursor: default; }.match-search-form { display: grid; grid-template-columns: 1fr 1fr 1fr auto; align-items: end; gap: 10px; padding: 12px 0; }.match-search-form label { min-width: 0; display: grid; gap: 5px; color: #777780; font-size: 11px; }.match-search-form input { width: 100%; height: 31px; box-sizing: border-box; border: 1px solid #cfcfd5; border-radius: 4px; padding: 0 8px; background: white; color: #33333a; }.search-button, .apply-lyrics-button { display: flex; align-items: center; justify-content: center; gap: 6px; }.search-button { height: 31px; }.match-grid { min-height: 0; flex: 1; display: grid; grid-template-columns: minmax(0, 1.1fr) minmax(0, .9fr); gap: 16px; }.match-results, .lyrics-preview-pane { min-height: 0; display: flex; flex-direction: column; border-top: 1px solid #d8d8dd; }.pane-heading { height: 34px; flex: 0 0 34px; display: flex; align-items: center; justify-content: space-between; color: #55555e; font-size: 12px; font-weight: 650; }.pane-heading small { min-width: 0; max-width: 70%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #92929a; font-weight: 400; }.match-results { overflow-y: auto; }.candidate-row { min-height: 48px; display: grid; grid-template-columns: minmax(120px, 1.25fr) minmax(90px, .9fr) minmax(90px, 1fr) 42px; align-items: center; gap: 8px; border: 0; border-bottom: 1px solid #e1e1e5; padding: 6px 8px; background: transparent; color: #73737c; text-align: left; cursor: pointer; font-size: 11px; }.candidate-row:hover { background: #ececf0; }.candidate-row.selected { background: #fff; box-shadow: inset 3px 0 #d13c86; color: #4d4d55; }.candidate-row > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.candidate-title { color: #33333a; font-size: 12px; font-weight: 650; }.candidate-row time { text-align: right; color: #92929a; }.match-placeholder { flex: 1; display: grid; place-items: center; color: #96969e; font-size: 12px; }.lyrics-preview { min-height: 0; flex: 1; resize: none; box-sizing: border-box; width: 100%; border: 1px solid #cfcfd5; border-radius: 4px; padding: 10px; background: #fff; color: #33333a; font: 12px/1.6 "Microsoft YaHei UI", sans-serif; user-select: text; white-space: pre-wrap; }.apply-lyrics-button { min-height: 32px; margin-top: 9px; align-self: flex-end; }
.number-input { width: 58px; padding: 7px 8px; border: 1px solid #cfcfd5; border-radius: 5px; text-align: center; }.offset-input { width: 108px; padding: 7px 8px; border: 1px solid #cfcfd5; border-radius: 5px; text-align: right; }.about-panel { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; color: #55555e; }.about-logo { width: 76px; height: 76px; border-radius: 18px; object-fit: cover; }.settings-footer { padding-top: 14px; display: flex; justify-content: space-between; align-items: center; }.save-message { color: #bf357c; font-size: 12px; }.footer-actions { display: flex; gap: 9px; }.button { min-width: 70px; height: 32px; border-radius: 5px; padding: 0 14px; font-size: 13px; cursor: pointer; }.button.secondary { border: 1px solid #d0d0d5; background: white; color: #5d5d65; }.button.primary { border: 0; background: #d13c86; color: white; }
</style>
