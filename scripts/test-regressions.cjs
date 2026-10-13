// 在 Node 中执行真实 Vue <script setup>，只模拟 Tauri 边界，验证启动和事件时序。
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const ts = require('typescript');
const vue = require('vue');
const root = path.resolve(__dirname, '..');
const theme = {};
new Function('exports', ts.transpileModule(fs.readFileSync(path.join(root, 'src/theme.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText)(theme);

function component(file, names, commands, label) {
  let mounted;
  const events = new Map();
  const source = fs.readFileSync(path.join(root, file), 'utf8').match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1];
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } }).outputText;
  const requireBoundary = (name) => {
    if (name === 'vue') return { ...vue, onMounted(fn) { mounted = fn; }, onUnmounted() {} };
    if (name === '../theme') return theme;
    if (name.startsWith('@tauri-apps/')) return {
      getCurrentWindow: () => ({ label }),
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
  const exposed = new Function('require', 'exports', '__APP_VERSION__', 'performance', 'document', 'requestAnimationFrame', js + '\nreturn {' + names.join(',') + '};')(requireBoundary, {}, 'test', { now: () => 0 }, { querySelector: () => style }, () => 1);
  return { ...exposed, mount: () => mounted(), event: (name, payload) => events.get(name)({ payload }) };
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
})().catch((error) => { console.error(error); process.exitCode = 1; });
