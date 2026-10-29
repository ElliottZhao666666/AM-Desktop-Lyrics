import type { LocalizedLyricText, LyricsAuxiliaryMode, LyricsDocument } from './types';

export interface AuxiliaryAvailability { translation: boolean; pronunciation: boolean }

export function auxiliaryAvailability(document: LyricsDocument | null): AuxiliaryAvailability {
  return {
    translation: !!document?.lines.some(line => line.translations?.some(value => value.text.trim())),
    pronunciation: !!document?.lines.some(line => line.pronunciations?.some(value => value.text.trim())),
  };
}

export function preferredAuxiliaryMode(preference: LyricsAuxiliaryMode, available: AuxiliaryAvailability): LyricsAuxiliaryMode {
  if (preference === 'off') return 'off';
  if (available[preference]) return preference;
  return available.translation ? 'translation' : available.pronunciation ? 'pronunciation' : 'off';
}

export function localizedLine(items: LocalizedLyricText[] | undefined, preferChinese = false): LocalizedLyricText | null {
  const available = items?.filter(value => value.text.trim()) ?? [];
  if (!preferChinese) return available[0] ?? null;
  const normalized = (language: string) => language.toLowerCase().replace(/_/g, '-');
  return available.find(value => /^zh-(hans|cn)/.test(normalized(value.language)))
    ?? available.find(value => normalized(value.language).startsWith('zh'))
    ?? available[0] ?? null;
}

export function auxiliaryFontSize(primarySize: number): number {
  return Math.max(12, (Number.isFinite(primarySize) ? primarySize : 32) - 8);
}
