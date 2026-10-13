// 在 Node 中执行真实 Vue <script setup>，只模拟 Tauri 边界，验证启动和事件时序。
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const ts = require('typescript');
const vue = require('vue');
const root = path.resolve(__dirname, '..');
const theme = {};
new Function('exports', 'require', ts.transpileModule(fs.readFileSync(path.join(root, 'src/theme.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText)(theme, () => ({ default: fs.readFileSync(path.join(root, 'src/default-theme.css'), 'utf8') }));
if (process.argv.includes('--write-default-theme')) {
  fs.writeFileSync(path.join(root, 'src/default-theme.css'), theme.buildThemeCss(theme.DEFAULT_EDITOR));
  process.exit(0);
}

function component(file, names, commands, label, props = {}) {
  let mounted;
  const events = new Map();
  const source = fs.readFileSync(path.join(root, file), 'utf8').match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1];
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText;
  const requireBoundary = (name) => {
    if (name === 'vue') return { ...vue, onMounted(fn) { mounted = fn; }, onUnmounted() {} };
    if (name === '../theme') return theme;
    if (name.startsWith('@tauri-apps/')) return {
      getCurrentWindow: () => ({ label, hide: async () => {} }),
      message: async () => {},
      invoke: async (command, args) => {
        if (!(command in commands)) throw Error(`Unexpected command: ${command}`);
        return typeof commands[command] === 'function' ? commands[command](args) : commands[command];
      },
      listen: async (event, listener) => { events.set(event, listener); return () => events.delete(event); },
      check: async () => null,
    };
    return {};
  };
  const style = { textContent: '' };
  const exposed = new Function('require', 'exports', '__APP_VERSION__', 'performance', 'document', 'requestAnimationFrame', 'defineProps', js + '\nreturn {' + names.join(',') + '};')(requireBoundary, {}, 'test', { now: () => 0 }, { querySelector: () => style }, () => 1, () => props);
  return { ...exposed, mount: () => mounted?.(), event: (name, payload) => events.get(name)({ payload }) };
}

const track = { title: 'A', artist: 'Artist', album: 'Album' };
const document = { track, source: 'qqMusic', duration_ms: 180000, lines: [{ begin_ms: 1000, end_ms: 2000, words: [], text: 'Lyric' }] };
const snapshot = {
  media: { track, playback_state: 'Playing', track_revision: 1, revision: 1 },
  position: { position_ms: 10000, duration_ms: 180000, playback_rate: 1, track_revision: 1, revision: 2 },
  lyrics: { document, status: { track, state: 'loaded', message: null }, track_revision: 1, revision: 3 },
};
const settings = { ...theme.DEFAULT_EDITOR, themeCss: '.amdl-lyrics {}', themeName: 'Test', lyricsTimeOffsetMs: -1500, localCacheLimit: 10 };

(async () => {
  const app = component('src/App.vue', ['currentLyrics', 'lyricsState', 'lyricsTimeOffsetMs', 'positionMs', 'displayPositionMs', 'adjustLyricsOffset', 'applyMediaState', 'applyLyricsSnapshot'], {
    get_lock_state: false, get_settings: settings,
    get_runtime_lyrics_time_offset: { offsetMs: -1500, revision: 1 }, get_media_snapshot: snapshot,
    adjust_runtime_lyrics_time_offset: ({ deltaMs }) => ({ offsetMs: -1500 + deltaMs, revision: 2 }),
  }, 'main');
  await app.mount();
  assert.equal(app.lyricsTimeOffsetMs.value, -1500);
  assert.equal(app.currentLyrics.value.lines[0].text, 'Lyric');
  assert.equal(app.lyricsState.value, 'loaded');
  assert.equal(app.positionMs.value, 10000);
  assert.equal(app.displayPositionMs.value, 11500);
  await app.adjustLyricsOffset(-500);
  assert.equal(app.lyricsTimeOffsetMs.value, -2000);
  app.event('lyrics-time-offset-changed', { offsetMs: 0, revision: 0 });
  assert.equal(app.lyricsTimeOffsetMs.value, -2000);
  console.log('通过：启动恢复 -1500ms、提前后 -2000ms、媒体时间独立、过期偏移被拒绝。');

  app.applyMediaState({ track: { ...track, album: 'Different Album' }, playback_state: 'Playing', track_revision: 2, revision: 4 });
  assert.equal(app.currentLyrics.value, null);
  app.applyLyricsSnapshot({ ...snapshot.lyrics, revision: 100 });
  assert.equal(app.currentLyrics.value, null);
  app.applyMediaState(snapshot.media);
  app.event('media-position-changed', { ...snapshot.position, revision: 101 });
  assert.equal(app.positionMs.value, 0);
  app.applyMediaState({ track, playback_state: 'Playing', track_revision: 3, revision: 102 });
  app.applyLyricsSnapshot(snapshot.lyrics);
  assert.equal(app.currentLyrics.value, null);
  app.applyLyricsSnapshot({ ...snapshot.lyrics, track_revision: 3, revision: 103 });
  app.applyLyricsSnapshot({ ...snapshot.lyrics, document: null, status: { track, state: 'loading', message: null }, track_revision: 3, revision: 102 });
  assert.equal(app.lyricsState.value, 'loaded');
  console.log('通过：启动歌词快照、不同专辑、A→B→A、旧进度与旧加载状态隔离。');

  const panel = component('src/components/SettingsView.vue', ['settings', 'lyricsOffsetInput'], {
    get_settings: settings, get_config: { lyricsSources: [], amApiTokens: {}, autoChs: true },
    get_runtime_lyrics_time_offset: { offsetMs: -1500, revision: 1 },
    get_system_fonts: () => { throw Error('模拟字体枚举失败'); }, get_media_snapshot: snapshot,
  }, 'settings');
  await panel.mount();
  assert.equal(panel.settings.value.lyricsTimeOffsetMs, -1500);
  assert.equal(panel.lyricsOffsetInput.value, '-1500');
  console.log('通过：字体加载失败不阻断设置面板恢复。');

  assert.equal(fs.readFileSync(path.join(root, 'src/default-theme.css'), 'utf8'), theme.buildThemeCss(theme.DEFAULT_EDITOR));
  for (const preset of theme.PRESETS) {
    const css = theme.buildThemeCss(preset.editor);
    assert.deepEqual(theme.restoreThemeEditor(css, preset.editor), preset.editor);
    const parsed = theme.parseThemeCss(css);
    assert.equal(parsed.fontSize, preset.editor.fontSize);
    assert.equal(parsed.active.mode, preset.editor.active.mode);
    assert.equal(parsed.active.color, preset.editor.active.color);
  }
  const legacyCss = '.amdl-lyrics {font-family:"My Custom Font", sans-serif;font-size:42px;font-weight:700;letter-spacing:2px;}\n.amdl-lyric-line.is-active {background:linear-gradient(90deg, red, green, blue);}\n.unrelated {margin:7px;}';
  const original = theme.parseThemeCss(legacyCss);
  assert.equal(original.fontFamily, 'My Custom Font');
  const changed = theme.cloneEditor(original); changed.fontSize = 48;
  const patched = theme.mergeThemeCss(legacyCss, original, changed);
  assert.match(patched, /font-size: 48px/);
  assert.match(patched, /red, green, blue/);
  assert.match(patched, /letter-spacing:2px/);
  assert.match(patched, /\.unrelated \{margin:7px;\}/);
  console.log('通过：主题保存恢复、旧 CSS 解析、三色渐变和未知规则在单项调整后保留。');

  let offset = { offsetMs: -1500, revision: 1 };
  let externalDuringRollback = false;
  let persisted = { ...settings, themeCss: legacyCss };
  const requests = [];
  const editorPanel = component('src/components/SettingsView.vue', ['settings','editor','fontChoice','customFontFamily','syncCustomFont','syncCss','discardEdits','updateLyricsOffsetInput','lyricsOffsetInput','searchLyrics','selectCandidate','matchBusy','searchBusy','previewBusy','matchCandidates','previewText','updateCurrentTrack','reloadSettings','windowLifecycle'], {
    get_settings: () => structuredClone(persisted), get_config: { lyricsSources: [], amApiTokens: {}, autoChs: true },
    get_runtime_lyrics_time_offset: () => offset,
    set_runtime_lyrics_time_offset: ({offsetMs,expectedRevision}) => {
      if (externalDuringRollback && expectedRevision !== undefined) { offset = {offsetMs:900,revision:offset.revision+1}; externalDuringRollback = false; }
      if (expectedRevision !== undefined && expectedRevision !== offset.revision) return offset;
      return (offset = { offsetMs, revision: offset.revision + 1 });
    },
    get_system_fonts: ['Microsoft YaHei UI'], get_media_snapshot: snapshot,
    search_qq_lyrics: () => new Promise((resolve,reject) => requests.push({resolve,reject})),
    preview_qq_lyrics: () => new Promise((resolve,reject) => requests.push({resolve,reject})),
  }, 'settings');
  await editorPanel.mount();
  assert.equal(editorPanel.editor.value.fontSize, 42);
  assert.equal(editorPanel.fontChoice.value, 'custom');
  for (const text of ['霞', '霞鹜', '霞鹜文楷']) {
    editorPanel.customFontFamily.value = text; editorPanel.syncCustomFont();
    assert.equal(editorPanel.fontChoice.value, 'custom'); assert.equal(editorPanel.editor.value.fontFamily, text);
  }
  assert.match(editorPanel.settings.value.themeCss, /red, green, blue/);
  editorPanel.updateLyricsOffsetInput({target:{value:'-2000'}});
  await editorPanel.discardEdits();
  assert.equal(offset.offsetMs, -1500);
  assert.equal(editorPanel.settings.value.themeCss, legacyCss);
  editorPanel.updateLyricsOffsetInput({target:{value:'-2500'}});
  await Promise.resolve(); await Promise.resolve();
  offset = {offsetMs:500,revision:100}; editorPanel.event('lyrics-time-offset-changed', offset);
  await editorPanel.discardEdits();
  assert.equal(offset.offsetMs, 500);
  editorPanel.updateLyricsOffsetInput({target:{value:'-3000'}});
  await Promise.resolve(); await Promise.resolve();
  externalDuringRollback = true;
  await editorPanel.discardEdits();
  assert.equal(offset.offsetMs, 900);
  persisted = {...persisted, themeCss:patched};
  editorPanel.event('settings-opened');
  await new Promise(setImmediate);
  assert.equal(editorPanel.editor.value.fontSize, 48);
  console.log('通过：自定义字体连续输入、取消回滚、外部偏移不被回滚、再次打开恢复最新主题。');

  const staleSearch = editorPanel.searchLyrics(); const oldSearch = requests.shift();
  editorPanel.updateCurrentTrack({...track,title:'B'});
  oldSearch.resolve([{songMid:'old'}]); await staleSearch;
  assert.equal(editorPanel.matchCandidates.value.length, 0);
  const candidate = {songMid:'one'};
  const firstPreview = editorPanel.selectCandidate(candidate); const firstRequest = requests.shift();
  const secondPreview = editorPanel.selectCandidate({songMid:'two'}); const secondRequest = requests.shift();
  firstRequest.resolve({lyricsText:'old'}); await firstPreview;
  assert.equal(editorPanel.previewBusy.value, true);
  assert.equal(editorPanel.previewText.value, '');
  secondRequest.resolve({lyricsText:'new'}); await secondPreview;
  assert.equal(editorPanel.previewText.value, 'new'); assert.equal(editorPanel.matchBusy.value, false);
  console.log('通过：切歌后的旧搜索不会回填，旧预览完成不会清掉新预览的忙碌状态。');

  const props = vue.reactive({title:'A',artist:'Artist',isPlaying:true,positionMs:1500,durationMs:5000,lyrics:structuredClone(document),lyricsState:'loaded',lyricsMessage:null});
  const board = component('src/components/LyricsBoard.vue', ['currentLines','displayLines','displayMode','wordProgress'], {}, 'main', props);
  assert.equal(board.currentLines.value.length, 1);
  props.positionMs = 2500;
  assert.equal(board.displayMode.value, 'outro'); assert.equal(board.displayLines.value.length, 0);
  props.lyrics.lines = [{begin_ms:1000,end_ms:null,text:'A',words:[]},{begin_ms:1000,end_ms:2500,text:'B',words:[]},{begin_ms:3000,end_ms:null,text:'C',words:[]}];
  props.positionMs = 2000; assert.equal(board.currentLines.value.length, 2);
  props.positionMs = 5000; assert.equal(board.displayMode.value, 'outro');
  assert.equal(board.wordProgress({begin_ms:1000,end_ms:2000}), 1);
  console.log('通过：重叠行和缺失结束时间可定位，最终结束后不残留最后一行。');
})().catch((error) => { console.error(error); process.exitCode = 1; });
