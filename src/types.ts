export type PlaybackState = 'Playing' | 'Paused' | 'Stopped' | 'Unknown';

export interface TrackIdentity {
  title: string;
  artist: string;
  album: string | null;
}

export type LyricsSource = 'appleMusicApi' | 'appleMusicCache' | 'qqMusic' | 'amllDb' | 'dummy';

export interface LyricWord {
  text: string;
  begin_ms: number;
  end_ms: number;
}

export interface LyricLine {
  begin_ms: number;
  end_ms: number | null;
  words: LyricWord[];
  text: string;
}

export interface LyricsDocument {
  track: TrackIdentity;
  source: LyricsSource;
  lines: LyricLine[];
  duration_ms: number | null;
}

export interface MediaStatePayload {
  track: TrackIdentity;
  playback_state: PlaybackState;
  track_revision: number;
  revision: number;
}

export interface MediaPositionPayload {
  position_ms: number;
  duration_ms: number;
  playback_rate: number;
  track_revision: number;
  revision: number;
}

export interface MediaSnapshotPayload {
  media: MediaStatePayload;
  position: MediaPositionPayload;
  lyrics: LyricsSnapshotPayload;
}

export interface LyricsSnapshotPayload {
  document: LyricsDocument | null;
  status: LyricsStatusPayload | null;
  track_revision: number;
  revision: number;
}

export type LyricsLoadState = 'loading' | 'loaded' | 'notFound' | 'error';

export interface LyricsStatusPayload {
  track: TrackIdentity;
  state: LyricsLoadState;
  message: string | null;
}

export interface LyricsWindowBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RuntimeLyricsTimeOffset {
  offsetMs: number;
  revision: number;
}

export interface AppSettings {
  autoConvertTraditionalToSimplified: boolean;
  localCacheLimit: number;
  themeName: string;
  themeCss: string;
  lyricsTimeOffsetMs: number;
  rememberLyricsWindowBounds: boolean;
  lyricsWindowBounds: LyricsWindowBounds | null;
}

export interface ThemeFile {
  name: string;
  css: string;
}

export interface QqSongCandidate {
  songId: number;
  songMid: string;
  track: TrackIdentity;
  durationMs: number | null;
}

export interface QqLyricsPreview {
  candidate: QqSongCandidate;
  lyricsText: string;
}

export interface LyricsSourceConfig {
  id: string;
  name: string;
  enabled: boolean;
  priority: number;
}

export interface AmApiTokens {
  authToken: string;
  mediaUserToken: string;
}

export interface AppConfig {
  lyricsSources: LyricsSourceConfig[];
  amApiTokens: AmApiTokens;
  autoChs: boolean;
}
