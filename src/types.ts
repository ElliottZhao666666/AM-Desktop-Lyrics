export type PlaybackState = 'Playing' | 'Paused' | 'Stopped' | 'Unknown';

export interface TrackIdentity {
  title: string;
  artist: string;
  album: string | null;
}

export type LyricsSource = 'appleMusicCache' | 'qqMusic' | 'amllDb' | 'dummy';

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
}

export interface MediaPositionPayload {
  position_ms: number;
  duration_ms: number;
}

export interface MediaSnapshotPayload {
  media: MediaStatePayload;
  position: MediaPositionPayload;
}

export type LyricsLoadState = 'loading' | 'loaded' | 'notFound' | 'error';

export interface LyricsStatusPayload {
  track: TrackIdentity;
  state: LyricsLoadState;
  message: string | null;
}