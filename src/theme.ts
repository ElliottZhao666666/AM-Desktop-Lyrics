export type PaintMode = 'solid' | 'gradient';
export type TextAlign = 'left' | 'center' | 'right';

export interface TextPaint {
  mode: PaintMode;
  color: string;
  colorEnd: string;
  angle: number;
  strokeEnabled: boolean;
  strokeWidth: number;
  strokeColor: string;
  shadowEnabled: boolean;
  shadowX: number;
  shadowY: number;
  shadowBlur: number;
  shadowColor: string;
  glowEnabled: boolean;
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
  strokeEnabled: false,
  strokeWidth: 0, strokeColor: '#000000',
  shadowEnabled: false,
  shadowX: 0, shadowY: 1, shadowBlur: 1, shadowColor: '#000000',
  glowEnabled: false,
  glowBlur: 0, glowColor,
});

export const DEFAULT_EDITOR: ThemeEditor = {
  fontFamily: 'Microsoft YaHei UI', fontSize: 32, fontWeight: 800,
  italic: false, align: 'center', opacity: 100,
  normal: paint('#d7d7de', '#ffffff', '#ffffff'),
  active: { ...paint('#fb7185', '#8b5cf6', '#d946ef'), mode: 'gradient' },
  progress: { mode: 'gradient', color: '#fb7185', colorEnd: '#d946ef', angle: 90 },
};

export const PRESETS = [
  {
    id: 'default',
    name: 'AMDL 默认',
    editor: {
      ...DEFAULT_EDITOR,
      normal: {
        ...DEFAULT_EDITOR.normal,
        strokeEnabled: false,
        shadowEnabled: false,
        glowEnabled: false,
      },
      active: {
        ...DEFAULT_EDITOR.active,
        strokeEnabled: false,
        shadowEnabled: false,
        glowEnabled: false,
      },
    },
  },
  {
    id: 'apple',
    name: 'Apple Music 红',
    editor: {
      ...DEFAULT_EDITOR,
      normal: {
        ...DEFAULT_EDITOR.normal,
        color: '#f3f4f6',
        strokeEnabled: false,
        shadowEnabled: false,
        glowEnabled: false,
      },
      active: {
        ...DEFAULT_EDITOR.active,
        color: '#ff375f',
        colorEnd: '#F57A93',
        strokeEnabled: false,
        shadowEnabled: false,
        glowEnabled: true,
        glowColor: 'rgba(255, 55, 95, 0.4)',
        glowBlur: 16,
      },
      progress: {
        ...DEFAULT_EDITOR.progress,
        color: '#ff375f',
        colorEnd: '#F57A93',
      },
    },
  },
  {
    id: 'ocean',
    name: '海蓝霓虹',
    editor: {
      ...DEFAULT_EDITOR,
      normal: {
        ...DEFAULT_EDITOR.normal,
        color: '#cbd5e1',
        strokeEnabled: false,
        shadowEnabled: false,
        glowEnabled: false,
      },
      active: {
        ...DEFAULT_EDITOR.active,
        color: '#22d3ee',
        colorEnd: '#3b82f6',
        strokeEnabled: false,
        shadowEnabled: false,
        glowEnabled: true,
        glowColor: 'rgba(34, 211, 238, 0.5)',
        glowBlur: 20,
      },
      progress: {
        ...DEFAULT_EDITOR.progress,
        color: '#22d3ee',
        colorEnd: '#3b82f6',
      },
    },
  },
  {
    id: 'mono',
    name: '纯白极简',
    editor: {
      ...DEFAULT_EDITOR,
      fontWeight: 700,
      normal: {
        ...DEFAULT_EDITOR.normal,
        color: '#a3a3a3',
        strokeEnabled: false,
        shadowEnabled: false,
        glowEnabled: false,
      },
      active: {
        ...DEFAULT_EDITOR.active,
        mode: 'solid',
        color: '#ffffff',
        strokeEnabled: false,
        shadowEnabled: true,
        shadowX: 0,
        shadowY: 2,
        shadowBlur: 0,
        shadowColor: 'rgba(0,0,0,0.3)',
        glowEnabled: false,
      },
      progress: {
        ...DEFAULT_EDITOR.progress,
        mode: 'solid',
        color: '#ffffff',
      },
    },
  },
  {
    id: 'orange',
    name: '暖橙日落',
    editor: {
      ...DEFAULT_EDITOR,
      normal: {
        ...DEFAULT_EDITOR.normal,
        color: '#f3e3d0',
        strokeEnabled: true,
        strokeWidth: 1,
        strokeColor: '#d97706',
        shadowEnabled: false,
        glowEnabled: false,
      },
      active: {
        ...DEFAULT_EDITOR.active,
        color: '#ff8a3d',
        colorEnd: '#ffd166',
        strokeEnabled: true,
        strokeWidth: 1,
        strokeColor: '#ea580c',
        shadowEnabled: false,
        glowEnabled: false,
      },
      progress: {
        ...DEFAULT_EDITOR.progress,
        color: '#ff8a3d',
        colorEnd: '#ffd166',
      },
    },
  },
  {
    id: 'green',
    name: '青绿森林',
    editor: {
      ...DEFAULT_EDITOR,
      normal: {
        ...DEFAULT_EDITOR.normal,
        color: '#d7eadf',
        strokeEnabled: true,
        strokeWidth: 1,
        strokeColor: '#15803d',
        shadowEnabled: false,
        glowEnabled: false,
      },
      active: {
        ...DEFAULT_EDITOR.active,
        color: '#34d399',
        colorEnd: '#a3e635',
        strokeEnabled: true,
        strokeWidth: 1,
        strokeColor: '#059669',
        shadowEnabled: false,
        glowEnabled: false,
      },
      progress: {
        ...DEFAULT_EDITOR.progress,
        color: '#34d399',
        colorEnd: '#a3e635',
      },
    },
  },
] as const;

function colorValue(paint: TextPaint | ThemeEditor['progress']) {
  return paint.mode === 'gradient'
    ? `linear-gradient(${paint.angle}deg, ${paint.color}, ${paint.colorEnd})`
    : paint.color;
}

function textRule(selector: string, paint: TextPaint) {
  const gradient = paint.mode === 'gradient';
  const shadows = [
    paint.shadowEnabled ? `${paint.shadowX}px ${paint.shadowY}px ${paint.shadowBlur}px ${paint.shadowColor}` : '',
    paint.glowEnabled && paint.glowBlur > 0 ? `0 0 ${paint.glowBlur}px ${paint.glowColor}` : '',
  ].filter(Boolean).join(', ');
  const effects = [
    paint.strokeEnabled ? `  -webkit-text-stroke: ${paint.strokeWidth}px ${paint.strokeColor};` : '',
    paint.shadowEnabled || paint.glowEnabled ? `  text-shadow: ${shadows || 'none'};` : '',
  ].filter(Boolean).join('\n');
  return `${selector} {
  color: ${gradient ? 'transparent' : paint.color};
  background: ${gradient ? colorValue(paint) : 'none'};
  background-clip: ${gradient ? 'text' : 'border-box'};
  -webkit-background-clip: ${gradient ? 'text' : 'border-box'};
  -webkit-text-fill-color: ${gradient ? 'transparent' : 'currentColor'};
${effects}
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
${textRule('.amdl-lyric-word', editor.normal)}
${textRule('.amdl-lyric-word-highlight.is-active', editor.active)}
.amdl-progress-fill { background: ${colorValue(editor.progress)}; }`;
}

export function cloneEditor(editor: ThemeEditor): ThemeEditor {
  return JSON.parse(JSON.stringify(editor)) as ThemeEditor;
}
