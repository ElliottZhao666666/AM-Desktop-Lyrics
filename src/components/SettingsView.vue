<template>
  <main class="settings-shell">
    <aside class="settings-sidebar">
      <div class="brand-mark">AMDL</div>
      <nav aria-label="设置分类">
        <button v-for="item in sections" :key="item.id" class="nav-item" :class="{ active: activeSection === item.id }" @click="activeSection = item.id">
          <component :is="item.icon" :size="17" /><span>{{ item.label }}</span>
        </button>
      </nav>
    </aside>

    <section class="settings-content">
      <header class="settings-header"><div><p class="eyebrow">AMDL</p><h1>{{ currentSection.label }}</h1></div></header>

      <div v-if="activeSection === 'general'" class="settings-list">
        <div class="setting-row"><div><strong>自动转换繁体歌词到简体</strong><p>Apple Music 缓存命中时自动转换华语歌词。</p></div><button class="switch" :class="{ on: settings.autoConvertTraditionalToSimplified }" role="switch" :aria-checked="settings.autoConvertTraditionalToSimplified" @click="settings.autoConvertTraditionalToSimplified = !settings.autoConvertTraditionalToSimplified"><span /></button></div>
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
                <label class="wide">字体<input v-model="editor.fontFamily" @input="syncCss" /></label>
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
              <div class="amdl-lyrics"><div class="amdl-lyrics-stack"><div class="amdl-lyric-line">昨日的风停在窗外</div><div class="amdl-lyric-line is-active">此刻听见你的声音</div></div></div>
              <div class="preview-progress"><div class="amdl-progress-fill" style="width: 62%" /></div>
            </div>
            <div class="source-header"><span>CSS 源码</span><button class="text-command" @click="resetDefault">恢复默认</button></div>
            <textarea v-model="settings.themeCss" class="css-editor" spellcheck="false" @input="selectedPreset = 'custom'" />
          </div>
        </div>
        <component :is="'style'">{{ settings.themeCss }}</component>
      </div>

      <div v-else class="about-panel"><div class="about-logo">AMDL</div><strong>Apple Music Desktop Lyrics</strong><span>桌面歌词工具</span><small>版本 0.1.0</small></div>

      <footer class="settings-footer"><span class="save-message">{{ message }}</span><div class="footer-actions"><button class="button secondary" @click="closeWindow">取消</button><button class="button primary" @click="saveSettings">应用</button></div></footer>
    </section>
  </main>
</template>

<script setup lang="ts">
import { computed, defineComponent, h, onMounted, ref, type PropType } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { FolderOpen, Info, Palette, Save, Settings2 } from '@lucide/vue';
import type { AppSettings, ThemeFile } from '../types';
import { buildThemeCss, cloneEditor, DEFAULT_EDITOR, PRESETS, type TextPaint, type ThemeEditor } from '../theme';

const TextStyleEditor = defineComponent({
  props: { title: { type: String, required: true }, paint: { type: Object as PropType<TextPaint>, required: true } },
  emits: ['change'],
  setup(props, { emit }) {
    const field = (text: string, child: ReturnType<typeof h>) => h('label', [text, child]);
    const input = (key: keyof TextPaint, attrs: Record<string, unknown>) => h('input', { ...attrs, value: props.paint[key], onInput: (event: Event) => { const target = event.target as HTMLInputElement; (props.paint as unknown as Record<string, unknown>)[key] = attrs.type === 'number' ? Number(target.value) : target.value; emit('change'); } });
    return () => h('section', { class: 'editor-section' }, [h('h2', props.title), h('div', { class: 'control-grid paint-controls' }, [
      field('填充', h('select', { value: props.paint.mode, onChange: (event: Event) => { props.paint.mode = (event.target as HTMLSelectElement).value as TextPaint['mode']; emit('change'); } }, [h('option', { value: 'solid' }, '纯色'), h('option', { value: 'gradient' }, '线性渐变')])),
      field('颜色', input('color', { type: 'color' })),
      ...(props.paint.mode === 'gradient' ? [field('结束色', input('colorEnd', { type: 'color' })), field('角度', input('angle', { type: 'number', min: 0, max: 360 }))] : []),
      field('描边', input('strokeWidth', { type: 'number', min: 0, max: 8, step: .5 })), field('描边色', input('strokeColor', { type: 'color' })),
      field('阴影 X', input('shadowX', { type: 'number', min: -30, max: 30 })), field('阴影 Y', input('shadowY', { type: 'number', min: -30, max: 30 })),
      field('阴影模糊', input('shadowBlur', { type: 'number', min: 0, max: 40 })), field('阴影色', input('shadowColor', { type: 'color' })),
      field('发光', input('glowBlur', { type: 'number', min: 0, max: 50 })), field('发光色', input('glowColor', { type: 'color' })),
    ])]);
  },
});

const sections = [{ id: 'general', label: '常用', icon: Settings2 }, { id: 'appearance', label: '外观', icon: Palette }, { id: 'about', label: '关于', icon: Info }] as const;
const activeSection = ref<typeof sections[number]['id']>('general');
const settings = ref<AppSettings>({ autoConvertTraditionalToSimplified: true, localCacheLimit: 10, themeName: 'AMDL 默认', themeCss: buildThemeCss(DEFAULT_EDITOR) });
const editor = ref<ThemeEditor>(cloneEditor(DEFAULT_EDITOR));
const selectedPreset = ref('default');
const message = ref('');
const currentSection = computed(() => sections.find((item) => item.id === activeSection.value) ?? sections[0]);

onMounted(async () => { try { settings.value = await invoke<AppSettings>('get_settings'); } catch (error) { message.value = String(error); } });

function syncCss() { settings.value.themeCss = buildThemeCss(editor.value); selectedPreset.value = 'custom'; }
function applyPreset() { const preset = PRESETS.find((item) => item.id === selectedPreset.value); if (!preset) return; editor.value = cloneEditor(preset.editor); settings.value.themeName = preset.name; settings.value.themeCss = buildThemeCss(editor.value); }
function resetDefault() { selectedPreset.value = 'default'; applyPreset(); }

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
async function saveSettings() {
  settings.value.localCacheLimit = Math.max(1, Math.min(50, Math.round(settings.value.localCacheLimit || 10)));
  try { await invoke('save_settings', { settings: settings.value }); message.value = '已应用'; setTimeout(() => { message.value = ''; }, 1800); } catch (error) { message.value = String(error); }
}
function closeWindow() { getCurrentWindow().close(); }
</script>

<style scoped>
.settings-shell { width: 100vw; height: 100vh; display: flex; overflow: hidden; color: #24242a; background: #f4f4f6; font-family: "Segoe UI", "Microsoft YaHei", sans-serif; }
.settings-sidebar { width: 176px; flex: 0 0 176px; padding: 28px 14px; box-sizing: border-box; background: #e9e9ed; border-right: 1px solid #d8d8dd; }.brand-mark { margin: 0 14px 30px; font-size: 18px; font-weight: 750; color: #d9428b; }.nav-item { width: 100%; height: 42px; display: flex; align-items: center; gap: 11px; border: 0; border-radius: 7px; padding: 0 13px; color: #55555e; background: transparent; font-size: 14px; cursor: pointer; }.nav-item:hover { background: #dedee4; }.nav-item.active { color: #bd2c76; background: #fff; box-shadow: 0 1px 3px #00000012; font-weight: 650; }
.settings-content { min-width: 0; flex: 1; padding: 28px 34px 22px; box-sizing: border-box; display: flex; flex-direction: column; }.settings-header { padding-bottom: 18px; border-bottom: 1px solid #dedee3; }.eyebrow { margin: 0 0 5px; color: #c13a7c; font-size: 11px; font-weight: 700; }.settings-header h1 { margin: 0; font-size: 22px; }.settings-list { padding-top: 8px; }.setting-row { min-height: 76px; display: flex; align-items: center; justify-content: space-between; gap: 20px; border-bottom: 1px solid #e1e1e5; }.setting-row strong { font-size: 14px; }.setting-row p { margin: 6px 0 0; color: #85858e; font-size: 12px; }.switch { width: 38px; height: 22px; padding: 2px; border: 0; border-radius: 12px; background: #c6c6cc; cursor: pointer; }.switch span { display: block; width: 18px; height: 18px; border-radius: 50%; background: white; transition: transform .18s; }.switch.on { background: #d53e8b; }.switch.on span { transform: translateX(16px); }
.appearance-workspace { min-height: 0; flex: 1; display: flex; flex-direction: column; }.appearance-toolbar { display: flex; align-items: end; gap: 14px; padding: 14px 0; border-bottom: 1px solid #dedee3; }.appearance-toolbar label { display: grid; gap: 5px; font-size: 11px; color: #777780; }.appearance-toolbar select { width: 150px; }.theme-name { width: 180px; }.file-actions { margin-left: auto; display: flex; gap: 6px; }.icon-command { height: 31px; display: flex; align-items: center; gap: 6px; border: 1px solid #ceced4; border-radius: 5px; padding: 0 10px; background: #fff; color: #55555e; cursor: pointer; }
.appearance-grid { min-height: 0; flex: 1; display: grid; grid-template-columns: minmax(390px, 1.1fr) minmax(300px, .9fr); gap: 24px; padding-top: 14px; }.editor-pane { min-height: 0; overflow-y: auto; padding-right: 8px; }.editor-section { padding: 0 0 13px; margin-bottom: 13px; border-bottom: 1px solid #dfdfe4; }.editor-section h2 { margin: 0 0 9px; font-size: 13px; color: #4d4d55; }.control-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 9px 11px; }.control-grid label { min-width: 0; display: grid; gap: 4px; color: #7b7b84; font-size: 11px; }.control-grid label.wide { grid-column: span 2; }.control-grid input:not([type="checkbox"]):not([type="range"]), .control-grid select, .appearance-toolbar input, .appearance-toolbar select { box-sizing: border-box; width: 100%; height: 29px; border: 1px solid #cfcfd5; border-radius: 4px; padding: 0 7px; background: white; color: #33333a; }.control-grid input[type="color"] { padding: 3px; }.control-grid input[type="range"] { width: 100%; }.check-label { align-content: end; grid-template-columns: auto 1fr !important; align-items: center; padding-bottom: 6px; }.range-value { color: #55555e; }.paint-controls { grid-template-columns: repeat(5, minmax(0, 1fr)); }
.preview-pane { min-height: 0; display: flex; flex-direction: column; }.preview-title, .source-header { height: 28px; display: flex; align-items: center; justify-content: space-between; color: #686871; font-size: 12px; font-weight: 650; }.theme-preview { position: relative; height: 176px; display: flex; align-items: center; overflow: hidden; background: #15151a; border: 1px solid #2e2e35; }.theme-preview::before { content: ''; position: absolute; inset: 0; background: linear-gradient(135deg, #23232b, #0d0d11 60%, #2b1823); }.theme-preview .amdl-lyrics { position: relative; width: 100%; padding: 20px; box-sizing: border-box; }.preview-progress { position: absolute; right: 0; bottom: 0; left: 0; height: 3px; background: #ffffff20; }.preview-progress > div { height: 100%; }.source-header { margin-top: 10px; }.text-command { border: 0; background: transparent; color: #bd2c76; cursor: pointer; }.css-editor { min-height: 110px; flex: 1; resize: none; border: 1px solid #cfcfd5; border-radius: 5px; padding: 10px; background: #fff; color: #33333a; font: 11px/1.5 Consolas, monospace; user-select: text; }
.number-input { width: 58px; padding: 7px 8px; border: 1px solid #cfcfd5; border-radius: 5px; text-align: center; }.about-panel { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; color: #55555e; }.about-logo { color: #d9428b; font-size: 35px; font-weight: 800; }.settings-footer { padding-top: 14px; display: flex; justify-content: space-between; align-items: center; }.save-message { color: #bf357c; font-size: 12px; }.footer-actions { display: flex; gap: 9px; }.button { min-width: 70px; height: 32px; border-radius: 5px; padding: 0 14px; font-size: 13px; cursor: pointer; }.button.secondary { border: 1px solid #d0d0d5; background: white; color: #5d5d65; }.button.primary { border: 0; background: #d13c86; color: white; }
</style>
