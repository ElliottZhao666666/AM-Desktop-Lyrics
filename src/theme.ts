import defaultThemeCss from './default-theme.css?raw';
export const DEFAULT_THEME_CSS = defaultThemeCss;
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
    `  -webkit-text-stroke: ${paint.strokeEnabled ? `${paint.strokeWidth}px ${paint.strokeColor}` : '0 transparent'};`,
    `  text-shadow: ${shadows || 'none'};`,
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
.amdl-progress-fill { background: ${colorValue(editor.progress)}; }`;
}

export function cloneEditor(editor: ThemeEditor): ThemeEditor {
  return JSON.parse(JSON.stringify(editor)) as ThemeEditor;
}

const THEME_SELECTORS = ['.amdl-lyrics', '.amdl-lyrics-stack', '.amdl-lyric-line', '.amdl-lyric-line.is-active', '.amdl-progress-fill'];
function declarations(body: string) {
  const values: Record<string, string> = {};
  const parts: string[] = [];
  let start = 0, depth = 0, quote = '', escaped = false;
  body = body.replace(/\/\*[\s\S]*?\*\//g, '');
  for (let index = 0; index < body.length; index++) {
    const char = body[index];
    if (escaped) { escaped = false; continue; }
    if (char === '\\') { escaped = true; continue; }
    if (quote) { if (char === quote) quote = ''; continue; }
    if (char === '"' || char === "'") quote = char;
    else if (char === '(') depth++;
    else if (char === ')') depth = Math.max(0, depth - 1);
    else if (char === ';' && depth === 0) { parts.push(body.slice(start, index)); start = index + 1; }
  }
  parts.push(body.slice(start));
  for (const part of parts) {
    const colon = part.indexOf(':');
    if (colon > 0) values[part.slice(0, colon).trim()] = part.slice(colon + 1).trim().replace(/\s*!important\s*$/, '');
  }
  return values;
}
function themeRules(css: string) {
  const rules: Record<string, Record<string, string>> = {};
  for (const match of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = match[1].replace(/\/\*[\s\S]*?\*\//g, '').trim();
    if (THEME_SELECTORS.includes(selector)) rules[selector] = { ...rules[selector], ...declarations(match[2]) };
  }
  return rules;
}
const splitColors = (value: string) => value.split(/,(?![^()]*\))/).map((part) => part.trim());
function restorePaint(paint: TextPaint | ThemeEditor['progress'], rule: Record<string, string>, progress = false) {
  const background = rule.background ?? rule['background-image'] ?? '';
  const gradient = background.match(/^linear-gradient\((.*)\)$/);
  if (gradient) {
    const stops = splitColors(gradient[1]);
    const angle = stops[0].match(/^(-?[\d.]+)deg$/);
    if (angle) { paint.angle = Number(angle[1]); stops.shift(); }
    if (stops.length >= 2) { paint.mode = 'gradient'; paint.color = stops[0]; paint.colorEnd = stops[stops.length - 1]; }
  } else {
    const color = progress ? background : rule.color;
    if (color && color !== 'transparent' && color !== 'none') { paint.mode = 'solid'; paint.color = color; }
  }
  if (!('strokeEnabled' in paint)) return;
  const stroke = rule['-webkit-text-stroke']?.match(/^([\d.]+)px\s+(.+)$/);
  paint.strokeEnabled = Boolean(stroke && Number(stroke[1]) > 0);
  if (paint.strokeEnabled && stroke) { paint.strokeWidth = Number(stroke[1]); paint.strokeColor = stroke[2]; }
  paint.shadowEnabled = false; paint.glowEnabled = false;
  for (const shadow of splitColors(rule['text-shadow'] ?? 'none')) {
    const parts = shadow.match(/^(-?[\d.]+)(?:px)?\s+(-?[\d.]+)(?:px)?\s+([\d.]+)(?:px)?\s+(.+)$/);
    if (!parts) continue;
    const [, x, y, blur, color] = parts;
    if (Number(x) === 0 && Number(y) === 0 && Number(blur) > 0) {
      paint.glowEnabled = true; paint.glowBlur = Number(blur); paint.glowColor = color;
    } else {
      paint.shadowEnabled = true; paint.shadowX = Number(x); paint.shadowY = Number(y); paint.shadowBlur = Number(blur); paint.shadowColor = color;
    }
  }
}
/** 旧主题恢复已知控件值，未知声明保留在原 CSS 中。 */
export function parseThemeCss(css: string): ThemeEditor {
  const editor = cloneEditor(DEFAULT_EDITOR);
  const rules = themeRules(css);
  const root = rules['.amdl-lyrics'] ?? {};
  const family = root['font-family']?.match(/^(?:"((?:\\.|[^"])*)"|'((?:\\.|[^'])*)'|([^,]+))/);
  if (family) editor.fontFamily = (family[1] ?? family[2] ?? family[3]).trim().replace(/\\(["'\\])/g, '$1');
  const number = (property: string, fallback: number) => Number.isFinite(parseFloat(root[property])) ? parseFloat(root[property]) : fallback;
  editor.fontSize = number('font-size', editor.fontSize);
  editor.fontWeight = number('font-weight', editor.fontWeight);
  editor.opacity = number('opacity', editor.opacity / 100) * 100;
  editor.italic = root['font-style'] === 'italic';
  if (['left', 'center', 'right'].includes(root['text-align'])) editor.align = root['text-align'] as TextAlign;
  restorePaint(editor.normal, rules['.amdl-lyric-line'] ?? {});
  restorePaint(editor.active, rules['.amdl-lyric-line.is-active'] ?? {});
  restorePaint(editor.progress, rules['.amdl-progress-fill'] ?? {}, true);
  return editor;
}

export function restoreThemeEditor(css: string, saved: unknown): ThemeEditor {
  if (saved && typeof saved === 'object') {
    const value = saved as Record<string, unknown>;
    const paintValid = (paint: unknown, text: boolean) => {
      if (!paint || typeof paint !== 'object') return false;
      const data = paint as Record<string, unknown>;
      if (!['solid', 'gradient'].includes(String(data.mode))) return false;
      if (!['color', 'colorEnd', ...(text ? ['strokeColor', 'shadowColor', 'glowColor'] : [])].every((name) => typeof data[name] === 'string' && !/[{};\n]/.test(String(data[name])))) return false;
      if (!['angle', ...(text ? ['strokeWidth', 'shadowX', 'shadowY', 'shadowBlur', 'glowBlur'] : [])].every((name) => typeof data[name] === 'number' && Number.isFinite(data[name]))) return false;
      return !text || ['strokeEnabled', 'shadowEnabled', 'glowEnabled'].every((name) => typeof data[name] === 'boolean');
    };
    if (typeof value.fontFamily === 'string' && typeof value.italic === 'boolean' && ['left', 'center', 'right'].includes(String(value.align))
      && ['fontSize', 'fontWeight', 'opacity'].every((name) => typeof value[name] === 'number' && Number.isFinite(value[name]))
      && paintValid(value.normal, true) && paintValid(value.active, true) && paintValid(value.progress, false)) return cloneEditor(saved as ThemeEditor);
  }
  return parseThemeCss(css);
}
/** 只更新控件改变的声明，保留自定义规则及其他属性。 */
export function mergeThemeCss(css: string, before: ThemeEditor, after: ThemeEditor) {
  const previous = themeRules(buildThemeCss(before));
  const next = themeRules(buildThemeCss(after));
  let result = css;
  for (const selector of THEME_SELECTORS) {
    const changes = Object.entries(next[selector] ?? {}).filter(([name, value]) => previous[selector]?.[name] !== value);
    if (selector === '.amdl-lyric-line.is-active') {
      for (const property of ['-webkit-text-stroke', 'text-shadow']) {
        if (previous['.amdl-lyric-line']?.[property] !== next['.amdl-lyric-line']?.[property] && !changes.some(([name]) => name === property)) changes.push([property, next[selector][property]]);
      }
    }
    if (!changes.length) continue;
    let replaced = false;
    result = result.replace(/([^{}]+)\{([^{}]*)\}/g, (whole, rawSelector: string, body: string) => {
      if (rawSelector.replace(/\/\*[\s\S]*?\*\//g, '').trim() !== selector) return whole;
      replaced = true;
      for (const [property, value] of changes) {
        const escaped = property.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        const pattern = new RegExp(`(^|;)\\s*${escaped}\\s*:[^;]*`, 'g');
        if (pattern.test(body)) body = body.replace(pattern, (_match, separator: string) => `${separator}\n  ${property}: ${value}`);
        else body = `${body.trimEnd()}${body.trimEnd().endsWith(';') ? '' : ';'}\n  ${property}: ${value};\n`;
      }
      return `${rawSelector}{${body}}`;
    });
    if (!replaced) result += `\n${selector} { ${changes.map(([name, value]) => `${name}: ${value};`).join(' ')} }`;
  }
  return result;
}
