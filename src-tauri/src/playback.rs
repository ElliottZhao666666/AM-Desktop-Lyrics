use crate::domain::*;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

static PLAYBACK: OnceLock<Mutex<PlaybackStore>> = OnceLock::new();

fn store() -> &'static Mutex<PlaybackStore> {
    PLAYBACK.get_or_init(|| Mutex::new(PlaybackStore::new()))
}

struct PlaybackStore {
    revision: u64,
    snapshot: MediaSnapshotPayload,
}

impl PlaybackStore {
    fn new() -> Self {
        Self {
            revision: 0,
            snapshot: MediaSnapshotPayload {
                media: MediaStatePayload {
                    track: TrackIdentity {
                        title: "等待播放...".into(),
                        artist: String::new(),
                        album: None,
                    },
                    playback_state: PlaybackState::Stopped,
                    track_revision: 0,
                    revision: 0,
                },
                position: MediaPositionPayload::default(),
                lyrics: LyricsSnapshotPayload {
                    document: None,
                    status: None,
                    track_revision: 0,
                    revision: 0,
                },
            },
        }
    }

    fn update_media(&mut self, mut media: MediaStatePayload) -> MediaStatePayload {
        self.revision += 1;
        let changed = self.snapshot.media.track != media.track;
        media.track_revision = self.snapshot.media.track_revision + u64::from(changed);
        media.revision = self.revision;
        if changed {
            self.snapshot.position = MediaPositionPayload {
                track_revision: media.track_revision,
                revision: self.revision,
                ..Default::default()
            };
            self.snapshot.lyrics = LyricsSnapshotPayload {
                track_revision: media.track_revision,
                revision: self.revision,
                document: None,
                status: None,
            };
        }
        self.snapshot.media = media.clone();
        media
    }

    fn update_position(
        &mut self,
        track_revision: u64,
        mut position: MediaPositionPayload,
    ) -> Option<MediaPositionPayload> {
        if track_revision != self.snapshot.media.track_revision {
            return None;
        }
        self.revision += 1;
        position.track_revision = track_revision;
        position.revision = self.revision;
        self.snapshot.position = position.clone();
        Some(position)
    }

    fn update_lyrics(
        &mut self,
        track_revision: u64,
        status: LyricsStatusPayload,
        document: Option<LyricsDocument>,
    ) -> Option<LyricsSnapshotPayload> {
        if track_revision != self.snapshot.media.track_revision
            || status.track != self.snapshot.media.track
        {
            return None;
        }
        self.revision += 1;
        self.snapshot.lyrics = LyricsSnapshotPayload {
            track_revision,
            revision: self.revision,
            status: Some(status),
            document,
        };
        Some(self.snapshot.lyrics.clone())
    }
}

pub fn snapshot() -> Result<MediaSnapshotPayload, String> {
    Ok(store()
        .lock()
        .map_err(|_| "媒体状态锁已损坏".to_string())?
        .snapshot
        .clone())
}

pub fn update_media(app: &AppHandle, media: MediaStatePayload) -> MediaStatePayload {
    let mut state = store().lock().unwrap_or_else(|error| error.into_inner());
    let media = state.update_media(media);
    let _ = app.emit("media-state-changed", &media);
    media
}

pub fn update_position(app: &AppHandle, track_revision: u64, position: MediaPositionPayload) {
    let mut state = store().lock().unwrap_or_else(|error| error.into_inner());
    if let Some(position) = state.update_position(track_revision, position) {
        let _ = app.emit("media-position-changed", position);
    }
}

pub fn update_lyrics(
    app: &AppHandle,
    track_revision: u64,
    status: LyricsStatusPayload,
    document: Option<LyricsDocument>,
) -> bool {
    let mut state = store().lock().unwrap_or_else(|error| error.into_inner());
    if let Some(lyrics) = state.update_lyrics(track_revision, status, document) {
        let _ = app.emit("lyrics-updated", lyrics);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn media(title: &str) -> MediaStatePayload {
        MediaStatePayload {
            track: TrackIdentity {
                title: title.into(),
                artist: "Artist".into(),
                album: None,
            },
            playback_state: PlaybackState::Playing,
            track_revision: 0,
            revision: 0,
        }
    }
    #[test]
    fn startup_snapshot_restores_lyrics_and_switch_rejects_late_results() {
        let mut state = PlaybackStore::new();
        let first = state.update_media(media("A"));
        let doc = LyricsDocument {
            storage: Default::default(),
            track: first.track.clone(),
            source: LyricsSource::QqMusic,
            lines: Vec::new(),
            duration_ms: None,
        };
        let status = LyricsStatusPayload {
            track: first.track.clone(),
            state: LyricsLoadState::Loaded,
            message: None,
        };
        state
            .update_lyrics(first.track_revision, status.clone(), Some(doc.clone()))
            .unwrap();
        assert_eq!(state.snapshot.lyrics.document, Some(doc.clone()));
        let second = state.update_media(media("B"));
        assert!(state.snapshot.lyrics.document.is_none());
        assert!(state
            .update_lyrics(first.track_revision, status, Some(doc))
            .is_none());
        assert!(state
            .update_position(first.track_revision, MediaPositionPayload::default())
            .is_none());
        let third = state.update_media(media("A"));
        assert!(third.track_revision > second.track_revision);
    }
    #[test]
    fn pause_does_not_create_a_new_track_revision() {
        let mut state = PlaybackStore::new();
        let playing = state.update_media(media("A"));
        let mut paused = media("A");
        paused.playback_state = PlaybackState::Paused;
        let paused = state.update_media(paused);
        assert_eq!(playing.track_revision, paused.track_revision);
        assert!(paused.revision > playing.revision);
    }
}
