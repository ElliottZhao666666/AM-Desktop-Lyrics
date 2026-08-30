export type PaintMode = 'solid' | 'gradient';
export type TextAlign = 'left' | 'center' | 'right';

export interface TextPaint {
  mode: PaintMode;
  color: string;
  colorEnd: string;
  angle: number;
  strokeWidth: number;
  strokeColor: string;
  shadowX: number;
  shadowY: number;
  shadowBlur: number;
  shadowColor: string;
  glowBlur: number;
  glowColor: string;
}

export interface ThemeEditor {
  fontFamily: string;
  fontSize: number;
  fontWeight: number;
  italic: boolean;
  align: TextAlign;
  opacity: number;
  normal: TextPaint;
  active: TextPaint;
  progress: Pick<TextPaint, 'mode' | 'color' | 'colorEnd' | 'angle'>;
}

const paint = (color: string, colorEnd: string, glowColor: string): TextPaint => ({
  mode: 'solid', color, colorEnd, angle: 90,
  strokeWidth: 0, strokeColor: '#000000',
  shadowX: 0, shadowY: 1, shadowBlur: 1, shadowColor: '#000000',
  glowBlur: 0, glowColor,
});

export const DEFAULT_EDITOR: ThemeEditor = {
  fontFamily: 'Microsoft YaHei UI', fontSize: 32, fontWeight: 800,
  italic: false, align: 'center', opacity: 100,
  normal: paint('#d7d7de', '#ffffff', '#ffffff'),
  active: { ...paint('#fb7185', '#8b5cf6', '#d946ef'), mode: 'gradient', shadowBlur: 1, glowBlur: 2 },
  progress: { mode: 'gradient', color: '#fb7185', colorEnd: '#d946ef', angle: 90 },
};

export const PRESETS = [
  { id: 'default', name: 'AMDL 默认', editor: DEFAULT_EDITOR },
  { id: 'apple', name: 'Apple Music 红', editor: { ...DEFAULT_EDITOR, normal: { ...DEFAULT_EDITOR.normal, color: '#f3f4f6' }, active: { ...DEFAULT_EDITOR.active, color: '#ff375f', colorEnd: '#ff9f0a', glowColor: '#ff375f' }, progress: { ...DEFAULT_EDITOR.progress, color: '#ff375f', colorEnd: '#ff9f0a' } } },
  { id: 'ocean', name: '海蓝霓虹', editor: { ...DEFAULT_EDITOR, normal: { ...DEFAULT_EDITOR.normal, color: '#cbd5e1' }, active: { ...DEFAULT_EDITOR.active, color: '#22d3ee', colorEnd: '#3b82f6', glowColor: '#22d3ee' }, progress: { ...DEFAULT_EDITOR.progress, color: '#22d3ee', colorEnd: '#3b82f6' } } },
  { id: 'mono', name: '纯白极简', editor: { ...DEFAULT_EDITOR, fontWeight: 700, normal: { ...DEFAULT_EDITOR.normal, color: '#a3a3a3', shadowBlur: 1 }, active: { ...DEFAULT_EDITOR.active, mode: 'solid', color: '#ffffff', glowBlur: 0, shadowBlur: 1 }, progress: { ...DEFAULT_EDITOR.progress, mode: 'solid', color: '#ffffff' } } },
  { id: 'orange', name: '暖橙日落', editor: { ...DEFAULT_EDITOR, normal: { ...DEFAULT_EDITOR.normal, color: '#f3e3d0', shadowBlur: 1 }, active: { ...DEFAULT_EDITOR.active, color: '#ff8a3d', colorEnd: '#ffd166', glowColor: '#ff9f43', shadowBlur: 1, glowBlur: 2 }, progress: { ...DEFAULT_EDITOR.progress, color: '#ff8a3d', colorEnd: '#ffd166' } } },
  { id: 'green', name: '青绿森林', editor: { ...DEFAULT_EDITOR, normal: { ...DEFAULT_EDITOR.normal, color: '#d7eadf', shadowBlur: 1 }, active: { ...DEFAULT_EDITOR.active, color: '#34d399', colorEnd: '#a3e635', glowColor: '#34d399', shadowBlur: 1, glowBlur: 2 }, progress: { ...DEFAULT_EDITOR.progress, color: '#34d399', colorEnd: '#a3e635' } } },
] as const;

function colorValue(paint: TextPaint | ThemeEditor['progress']) {
  return paint.mode === 'gradient'
    ? `linear-gradient(${paint.angle}deg, ${paint.color}, ${paint.colorEnd})`
    : paint.color;
}

function textRule(selector: string, paint: TextPaint) {
  const gradient = paint.mode === 'gradient';
  const shadows = [
    `${paint.shadowX}px ${paint.shadowY}px ${paint.shadowBlur}px ${paint.shadowColor}`,
    paint.glowBlur > 0 ? `0 0 ${paint.glowBlur}px ${paint.glowColor}` : '',
  ].filter(Boolean).join(', ');
  return `${selector} {
  color: ${gradient ? 'transparent' : paint.color};
  background: ${gradient ? colorValue(paint) : 'none'};
  background-clip: ${gradient ? 'text' : 'border-box'};
  -webkit-background-clip: ${gradient ? 'text' : 'border-box'};
  -webkit-text-fill-color: ${gradient ? 'transparent' : 'currentColor'};
  -webkit-text-stroke: ${paint.strokeWidth}px ${paint.strokeColor};
  text-shadow: ${shadows || 'none'};
}`;
}

export function buildThemeCss(editor: ThemeEditor) {
  return `.amdl-lyrics {
  font-family: "${editor.fontFamily.replace(/"/g, '\\"')}", "Segoe UI", sans-serif;
  font-size: ${editor.fontSize}px;
  font-weight: ${editor.fontWeight};
  font-style: ${editor.italic ? 'italic' : 'normal'};
  text-align: ${editor.align};
  opacity: ${editor.opacity / 100};
}
.amdl-lyrics-stack { width: 100%; }
${textRule('.amdl-lyric-line', editor.normal)}
${textRule('.amdl-lyric-line.is-active', editor.active)}
.amdl-progress-fill { background: ${colorValue(editor.progress)}; }`;
}

export function cloneEditor(editor: ThemeEditor): ThemeEditor {
  return JSON.parse(JSON.stringify(editor)) as ThemeEditor;
}
