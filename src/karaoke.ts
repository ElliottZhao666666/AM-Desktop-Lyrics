import type { LyricWord } from './types';

export interface GlyphRect { x: number; y: number; width: number; height: number; endsRow?: boolean }
export interface KaraokeGeometry { width: number; height: number; padding: number; words: GlyphRect[][] }

export function wordProgress(word: LyricWord, positionMs: number) {
  return Math.max(0, Math.min(1, (positionMs - word.begin_ms) / Math.max(1, word.end_ms - word.begin_ms)));
}

function rectPath(rect: GlyphRect) {
  const n = (value: number) => Math.round(value * 1000) / 1000;
  return `M${n(rect.x)} ${n(rect.y)}h${n(rect.width)}v${n(rect.height)}h${n(-rect.width)}Z`;
}

/** 两套字形共用整行坐标；未唱部分使用高亮区域的补集，避免双重描边。 */
export function karaokeMasks(words: LyricWord[], geometry: KaraokeGeometry, positionMs: number) {
  const rectangles: GlyphRect[] = [];
  let extent = 0;
  words.forEach((word, index) => {
    const regions = geometry.words[index] ?? [];
    let remaining = regions.reduce((sum, rect) => sum + rect.width, 0) * wordProgress(word, positionMs);
    for (const rect of regions) {
      const width = Math.min(rect.width, remaining);
      if (width <= 0) break;
      extent = Math.max(extent, rect.x + width);
      const left = rect.x <= 0.5 ? geometry.padding : 0;
      const right = rect.x + width >= geometry.width - 0.5 || (rect.endsRow && width === rect.width) ? geometry.padding : 0;
      rectangles.push({ ...rect, x: rect.x - left, width: width + left + right });
      remaining -= width;
    }
  });
  rectangles.sort((a, b) => a.y - b.y || a.x - b.x);
  const merged: GlyphRect[] = [];
  for (const rect of rectangles) {
    const previous = merged[merged.length - 1];
    if (previous && Math.abs(previous.y - rect.y) < 0.01 && Math.abs(previous.height - rect.height) < 0.01 && rect.x <= previous.x + previous.width + 0.01) {
      previous.width = Math.max(previous.x + previous.width, rect.x + rect.width) - previous.x;
    } else merged.push({ ...rect });
  }
  const active = merged.map(rectPath).join('') || 'M0 0Z';
  const normal = rectPath({ x: -geometry.padding, y: -geometry.padding, width: geometry.width + geometry.padding * 2, height: geometry.height + geometry.padding * 2 }) + (merged.length ? active : '');
  return { active, normal, extent: Math.min(geometry.width, extent) };
}
