use crate::domain::{
    MediaPositionPayload, MediaSnapshotPayload, MediaStatePayload, PlaybackState, TrackIdentity,
};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::command;
use windows::core::Interface;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
struct WinRtApartment;
impl WinRtApartment {
    fn initialize() -> windows::core::Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}
impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}
static CURRENT_SESSION: Mutex<Option<GlobalSystemMediaTransportControlsSession>> = Mutex::new(None);
fn convert_status(
    status: GlobalSystemMediaTransportControlsSessionPlaybackStatus,
) -> PlaybackState {
    match status {
        GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => PlaybackState::Playing,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => PlaybackState::Paused,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped
        | GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed => PlaybackState::Stopped,
        _ => PlaybackState::Unknown,
    }
}
fn find_apple_music_session(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
) -> windows::core::Result<Option<GlobalSystemMediaTransportControlsSession>> {
    let sessions = manager.GetSessions()?;
    for index in 0..sessions.Size()? {
        let session = sessions.GetAt(index)?;
        if session
            .SourceAppUserModelId()?
            .to_string()
            .to_lowercase()
            .contains("applemusic")
        {
            return Ok(Some(session));
        }
    }
    Ok(None)
}
fn waiting_state() -> MediaStatePayload {
    MediaStatePayload {
        track: TrackIdentity {
            title: "等待播放...".into(),
            artist: String::new(),
            album: None,
        },
        playback_state: PlaybackState::Stopped,
        track_revision: 0,
        revision: 0,
    }
}
fn is_unavailable_track(track: &TrackIdentity) -> bool {
    track.title.trim().is_empty()
        || track.artist.trim().is_empty()
        || track.title.contains("正在连接")
        || track.title == "等待播放..."
}
fn normalize_playable_state(payload: MediaStatePayload) -> MediaStatePayload {
    if is_unavailable_track(&payload.track) {
        waiting_state()
    } else {
        payload
    }
}
fn clear_current_session() {
    if let Ok(mut session) = CURRENT_SESSION.lock() {
        *session = None;
    }
}
fn enter_waiting(app: &tauri::AppHandle) {
    clear_current_session();
    crate::lyrics::provider::enter_waiting_state();
    let media = crate::playback::update_media(app, waiting_state());
    crate::playback::update_position(app, media.track_revision, MediaPositionPayload::default());
}
fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn metadata_segments(value: &str) -> Vec<&str> {
    value
        .split(|ch| matches!(ch, '—' | '–' | '－'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_apple_music_metadata(
    raw_title: String,
    raw_artist: String,
    raw_album: Option<String>,
) -> TrackIdentity {
    let title_segments = metadata_segments(&raw_title);
    if title_segments.len() >= 3 {
        return TrackIdentity {
            title: title_segments[0].to_string(),
            artist: title_segments[1].to_string(),
            album: Some(title_segments[2..].join(" — ")),
        };
    }

    let artist_segments = metadata_segments(&raw_artist);
    if artist_segments.len() >= 2 {
        let parsed_album = artist_segments[1..].join(" — ");
        let album = raw_album.or_else(|| Some(parsed_album));
        return TrackIdentity {
            title: raw_title.trim().to_string(),
            artist: artist_segments[0].to_string(),
            album,
        };
    }

    let artist = clean_artist(raw_artist, raw_album.as_deref());
    TrackIdentity {
        title: raw_title.trim().to_string(),
        artist,
        album: raw_album,
    }
}

fn clean_artist(artist: String, album: Option<&str>) -> String {
    let artist = artist.trim();
    let Some(album) = album.map(str::trim).filter(|value| !value.is_empty()) else {
        return artist.to_string();
    };

    artist
        .char_indices()
        .rev()
        .filter(|(_, ch)| matches!(ch, '-' | '—' | '–' | '－'))
        .find_map(|(index, ch)| {
            let suffix = artist[index + ch.len_utf8()..].trim();
            (normalize_metadata(suffix) == normalize_metadata(album)).then(|| &artist[..index])
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(artist)
        .to_string()
}

fn normalize_metadata(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 从会话中读取媒体属性和播放状态，打包为 MediaStatePayload。
/// 使用 TryGetMediaPropertiesAsync().get() 同步阻塞获取异步结果。
fn read_media_state(
    session: &GlobalSystemMediaTransportControlsSession,
) -> windows::core::Result<MediaStatePayload> {
    // TryGetMediaPropertiesAsync() 异步获取媒体元数据（标题、艺术家等），
    // .get() 阻塞等待异步操作完成（在事件回调和同步上下文中使用）
    let props = session.TryGetMediaPropertiesAsync()?.get()?;
    let raw_title = props.Title()?.to_string();
    let raw_album = non_empty(props.AlbumTitle()?.to_string());
    let raw_artist = props.Artist()?.to_string();
    let track =
        parse_apple_music_metadata(raw_title.clone(), raw_artist.clone(), raw_album.clone());
    println!(
        "[SMTC] 原始元数据: title={:?}, artist={:?}, album={:?}; 分离后 title={:?}, artist={:?}, album={:?}",
        raw_title, raw_artist, raw_album, track.title, track.artist, track.album
    );

    // GetPlaybackInfo() 同步获取播放信息，PlaybackStatus() 返回播放状态枚举
    let info = session.GetPlaybackInfo()?;
    let status = info.PlaybackStatus()?;
    let playback_state = convert_status(status);

    Ok(MediaStatePayload {
        track,
        playback_state,
        track_revision: 0,
        revision: 0,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TimelineSample {
    position_ms: u64,
    duration_ms: u64,
    updated_at: i64,
    rate: f64,
}
fn utc_ticks() -> i64 {
    const EPOCH: i64 = 116444736000000000;
    EPOCH
        + SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .saturating_div(100)
            .min((i64::MAX - EPOCH) as u128) as i64
}
fn estimate_position(sample: TimelineSample, playing: bool, now: i64) -> MediaPositionPayload {
    let elapsed = if playing && sample.updated_at > 0 {
        (now.saturating_sub(sample.updated_at)).max(0) as f64 / 10000.0 * sample.rate
    } else {
        0.0
    };
    let position = sample.position_ms.saturating_add(elapsed.round() as u64);
    MediaPositionPayload {
        position_ms: if sample.duration_ms > 0 {
            position.min(sample.duration_ms)
        } else {
            position
        },
        duration_ms: sample.duration_ms,
        playback_rate: sample.rate,
        track_revision: 0,
        revision: 0,
    }
}
fn read_timeline(
    session: &GlobalSystemMediaTransportControlsSession,
) -> windows::core::Result<TimelineSample> {
    let timeline = session.GetTimelineProperties()?;
    let start = timeline.StartTime()?.Duration;
    let end = timeline.EndTime()?.Duration;
    let rate = session
        .GetPlaybackInfo()?
        .PlaybackRate()
        .and_then(|rate| rate.Value())
        .unwrap_or(1.0);
    Ok(TimelineSample {
        position_ms: (timeline.Position()?.Duration - start).max(0) as u64 / 10000,
        duration_ms: (end - start).max(0) as u64 / 10000,
        updated_at: timeline.LastUpdatedTime()?.UniversalTime,
        rate: if rate.is_finite() && rate >= 0.0 {
            rate
        } else {
            1.0
        },
    })
}
#[derive(Clone, Copy)]
enum MediaEventKind {
    Metadata,
    Playback,
}
struct MediaEvent {
    session_generation: u64,
    kind: MediaEventKind,
    observed_at: i64,
}
struct SessionBinding {
    session: GlobalSystemMediaTransportControlsSession,
    props_token: i64,
    playback_token: i64,
    generation: u64,
}
impl Drop for SessionBinding {
    fn drop(&mut self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.props_token);
        let _ = self.session.RemovePlaybackInfoChanged(self.playback_token);
    }
}
fn bind_session(
    session: GlobalSystemMediaTransportControlsSession,
    generation: u64,
    sender: &mpsc::Sender<MediaEvent>,
) -> windows::core::Result<SessionBinding> {
    let props_sender = sender.clone();
    let props = TypedEventHandler::new(
        move |_session: windows::core::Ref<'_, GlobalSystemMediaTransportControlsSession>,
              _args| {
            let _ = props_sender.send(MediaEvent {
                session_generation: generation,
                kind: MediaEventKind::Metadata,
                observed_at: utc_ticks(),
            });
            Ok(())
        },
    );
    let playback_sender = sender.clone();
    let playback = TypedEventHandler::new(
        move |_session: windows::core::Ref<'_, GlobalSystemMediaTransportControlsSession>,
              _args| {
            let _ = playback_sender.send(MediaEvent {
                session_generation: generation,
                kind: MediaEventKind::Playback,
                observed_at: utc_ticks(),
            });
            Ok(())
        },
    );
    let props_token = session.MediaPropertiesChanged(&props)?;
    let playback_token = match session.PlaybackInfoChanged(&playback) {
        Ok(token) => token,
        Err(error) => {
            let _ = session.RemoveMediaPropertiesChanged(props_token);
            return Err(error);
        }
    };
    Ok(SessionBinding {
        session,
        props_token,
        playback_token,
        generation,
    })
}

pub struct SmtcManager {
    app_handle: Arc<tauri::AppHandle>,
}
impl SmtcManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle: Arc::new(app_handle),
        }
    }
    pub fn start(self) {
        std::thread::spawn(move || match WinRtApartment::initialize() {
            Ok(_apartment) => smtc_main_loop(self.app_handle),
            Err(error) => eprintln!("[SMTC] WinRT 初始化失败: {error}"),
        });
    }
}

// RPC 回调仅入队；读取、去抖、时间轴同步均在初始化过 WinRT 的单一线程执行。
fn smtc_main_loop(app: Arc<tauri::AppHandle>) {
    let (sender, receiver) = mpsc::channel();
    let mut generation = 0;
    loop {
        let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .and_then(|operation| operation.get())
        {
            Ok(manager) => manager,
            Err(error) => {
                eprintln!("[SMTC] 请求会话失败: {error}");
                enter_waiting(&app);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let mut binding: Option<SessionBinding> = None;
        let mut metadata_due: Option<(Instant, i64)> = None;
        let mut stale_sample: Option<TimelineSample> = None;
        let mut poll_due = Instant::now();
        let mut validate_due = Instant::now();
        loop {
            if binding.is_none() {
                match find_apple_music_session(&manager) {
                    Ok(Some(session)) => {
                        generation += 1;
                        match bind_session(session, generation, &sender) {
                            Ok(new_binding) => {
                                if let Ok(mut current) = CURRENT_SESSION.lock() {
                                    *current = Some(new_binding.session.clone());
                                }
                                binding = Some(new_binding);
                                metadata_due = Some((Instant::now(), 0));
                                poll_due = Instant::now();
                                stale_sample = None;
                            }
                            Err(error) => {
                                eprintln!("[SMTC] 注册事件失败: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                                continue;
                            }
                        }
                    }
                    Ok(None) => {
                        enter_waiting(&app);
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                    Err(error) => {
                        eprintln!("[SMTC] 枚举会话失败: {error}");
                        break;
                    }
                }
            }
            let connected = binding.as_ref().unwrap();
            let mut events = Vec::new();
            if let Ok(event) = receiver.recv_timeout(Duration::from_millis(100)) {
                events.push(event);
            }
            events.extend(receiver.try_iter());
            for event in events {
                if event.session_generation != connected.generation {
                    continue;
                }
                match event.kind {
                    MediaEventKind::Metadata => {
                        metadata_due = Some((
                            Instant::now() + Duration::from_millis(400),
                            event.observed_at,
                        ))
                    }
                    MediaEventKind::Playback => {
                        if let (Ok(info), Ok(snapshot)) = (
                            connected.session.GetPlaybackInfo(),
                            crate::playback::snapshot(),
                        ) {
                            if let Ok(status) = info.PlaybackStatus() {
                                let mut media = snapshot.media;
                                media.playback_state = convert_status(status);
                                crate::playback::update_media(&app, media);
                            }
                        }
                        poll_due = Instant::now();
                    }
                }
            }
            if metadata_due.is_some_and(|(deadline, _)| Instant::now() >= deadline) {
                let (_, observed_at) = metadata_due.take().unwrap();
                match (
                    read_media_state(&connected.session),
                    read_timeline(&connected.session),
                    read_media_state(&connected.session),
                ) {
                    (Ok(first), Ok(sample), Ok(second)) if first.track == second.track => {
                        let previous = crate::playback::snapshot().ok();
                        let changed = previous
                            .as_ref()
                            .is_none_or(|previous| previous.media.track != second.track);
                        if changed {
                            stale_sample = if observed_at > 0
                                && sample.updated_at.saturating_add(2_000_000) < observed_at
                            {
                                Some(sample)
                            } else {
                                None
                            };
                        }
                        let media =
                            crate::playback::update_media(&app, normalize_playable_state(second));
                        if is_unavailable_track(&media.track) {
                            crate::lyrics::provider::enter_waiting_state();
                        } else {
                            let position = if stale_sample == Some(sample) {
                                MediaPositionPayload::default()
                            } else {
                                estimate_position(
                                    sample,
                                    media.playback_state == PlaybackState::Playing,
                                    utc_ticks(),
                                )
                            };
                            let duration = position.duration_ms;
                            crate::playback::update_position(&app, media.track_revision, position);
                            crate::lyrics::provider::fetch_and_emit_lyrics(
                                media.track,
                                (*app).clone(),
                                duration,
                                media.track_revision,
                            );
                        }
                    }
                    (Err(error), _, _) => {
                        eprintln!("[SMTC] 读取元数据失败: {error}");
                        metadata_due =
                            Some((Instant::now() + Duration::from_millis(400), observed_at));
                    }
                    _ => {
                        metadata_due =
                            Some((Instant::now() + Duration::from_millis(200), observed_at))
                    }
                }
            }
            if metadata_due.is_none() && Instant::now() >= poll_due {
                poll_due = Instant::now() + Duration::from_secs(1);
                match (
                    read_timeline(&connected.session),
                    crate::playback::snapshot(),
                ) {
                    (Ok(sample), Ok(snapshot)) => {
                        if stale_sample.is_some_and(|stale| stale != sample) {
                            stale_sample = None;
                        }
                        if stale_sample != Some(sample) {
                            let position = estimate_position(
                                sample,
                                snapshot.media.playback_state == PlaybackState::Playing,
                                utc_ticks(),
                            );
                            let duration = position.duration_ms;
                            crate::playback::update_position(
                                &app,
                                snapshot.media.track_revision,
                                position,
                            );
                            if !is_unavailable_track(&snapshot.media.track) {
                                crate::lyrics::provider::fetch_and_emit_lyrics(
                                    snapshot.media.track,
                                    (*app).clone(),
                                    duration,
                                    snapshot.media.track_revision,
                                );
                            }
                        }
                    }
                    _ => {
                        binding = None;
                        metadata_due = None;
                        enter_waiting(&app);
                        continue;
                    }
                }
            }
            if Instant::now() >= validate_due {
                validate_due = Instant::now() + Duration::from_secs(4);
                match find_apple_music_session(&manager) {
                    Ok(Some(active)) if active.as_raw() == connected.session.as_raw() => {}
                    Ok(_) => {
                        binding = None;
                        metadata_due = None;
                        enter_waiting(&app);
                    }
                    Err(_) => break,
                }
            }
        }
        drop(binding);
        enter_waiting(&app);
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[command]
pub fn get_media_snapshot() -> Result<MediaSnapshotPayload, String> {
    crate::playback::snapshot()
}
async fn control_playback(action: &'static str) -> Result<(), String> {
    let session = CURRENT_SESSION
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or("没有活跃的 Apple Music 会话")?;
    let work = tauri::async_runtime::spawn_blocking(move || {
        let _apartment = WinRtApartment::initialize().map_err(|error| error.to_string())?;
        let operation = match action {
            "播放/暂停" => session.TryTogglePlayPauseAsync(),
            "下一曲" => session.TrySkipNextAsync(),
            _ => session.TrySkipPreviousAsync(),
        }
        .map_err(|error| error.to_string())?;
        let accepted = operation.get().map_err(|error| error.to_string())?;
        if !accepted {
            return Err(format!("Apple Music 拒绝了{action}操作"));
        }
        println!("[SMTC] 执行: {action}");
        Ok(())
    });
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .map_err(|_| format!("Apple Music {action}响应超时"))?
        .map_err(|error| error.to_string())?
}
#[command]
pub async fn play_pause() -> Result<(), String> {
    control_playback("播放/暂停").await
}
#[command]
pub async fn next_track() -> Result<(), String> {
    control_playback("下一曲").await
}
#[command]
pub async fn prev_track() -> Result<(), String> {
    control_playback("上一曲").await
}
#[cfg(test)]
mod tests {
    use super::{clean_artist, is_unavailable_track, parse_apple_music_metadata};

    #[test]
    #[ignore = "requires Apple Music playing and live catalog access; only reads playback"]
    fn matches_live_apple_cache_without_controlling_playback() {
        let _apartment = super::WinRtApartment::initialize().unwrap();
        let manager=windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager::RequestAsync().unwrap().get().unwrap();
        let session = super::find_apple_music_session(&manager)
            .unwrap()
            .expect("请先在 Apple Music 播放歌曲");
        let media = super::read_media_state(&session).unwrap();
        let sample = super::read_timeline(&session).unwrap();
        let estimated = super::estimate_position(
            sample,
            media.playback_state == crate::domain::PlaybackState::Playing,
            super::utc_ticks(),
        );
        println!(
            "[实机核对] track={:?}, raw={}ms, estimated={}ms, duration={}ms",
            media.track, sample.position_ms, estimated.position_ms, sample.duration_ms
        );
        let config = crate::config::AppConfig::default();
        let current = tauri::async_runtime::block_on(crate::lyrics::cache::fetch_lyrics(
            &media.track,
            std::time::SystemTime::now(),
            sample.duration_ms,
            &config,
        ))
        .unwrap();
        if let Some(candidate) = current {
            assert_eq!(candidate.document.track, media.track);
            println!("[实机核对] 当前曲目匹配成功: {}", candidate.track_id);
        } else {
            println!("[实机核对] 当前曲目没有匹配缓存，未采用上一首歌词");
        }
        let root = crate::lyrics::cache_scanner::cache_root().unwrap();
        let candidates = crate::lyrics::cache_scanner::scan_candidates(&root, 0).unwrap();
        let ids = candidates
            .iter()
            .map(|candidate| candidate.track_id.clone())
            .collect::<Vec<_>>();
        let metadata = tauri::async_runtime::block_on(crate::lyrics::am_api::fetch_song_metadata(
            &ids,
            &config.am_api_tokens,
        ))
        .unwrap();
        let cached_song = candidates
            .iter()
            .find_map(|candidate| metadata.get(&candidate.track_id))
            .expect("需要至少一份可核验的歌词缓存");
        let candidate = tauri::async_runtime::block_on(crate::lyrics::cache::fetch_lyrics(
            &cached_song.track,
            std::time::SystemTime::now(),
            cached_song.duration_ms.unwrap_or(0),
            &config,
        ))
        .unwrap()
        .expect("已核验缓存应能正确读取");
        assert_eq!(candidate.document.track, cached_song.track);
        assert!(candidate
            .document
            .lines
            .iter()
            .all(|line| line.end_ms.is_some_and(|end| end > line.begin_ms)));
        for line in &candidate.document.lines {
            if !line.words.is_empty() {
                assert_eq!(
                    line.words
                        .iter()
                        .map(|word| word.text.as_str())
                        .collect::<String>(),
                    line.text
                );
            }
        }
        println!(
            "[实机核对] 已核验 lyricsId={}, lines={}",
            candidate.track_id,
            candidate.document.lines.len()
        );
        let wrong = crate::domain::TrackIdentity {
            title: format!("{}__AMDL不存在的曲目", cached_song.track.title),
            ..cached_song.track.clone()
        };
        assert!(
            tauri::async_runtime::block_on(crate::lyrics::cache::fetch_lyrics(
                &wrong,
                std::time::SystemTime::now(),
                cached_song.duration_ms.unwrap_or(0),
                &config
            ))
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn rejects_empty_and_connecting_metadata() {
        assert!(is_unavailable_track(
            &crate::domain::TrackIdentity::default()
        ));
        assert!(is_unavailable_track(&crate::domain::TrackIdentity {
            title: "正在连接 Apple Music".into(),
            artist: "Apple Music".into(),
            album: None,
        }));
    }

    #[test]
    fn separates_artist_and_album_when_album_property_is_missing() {
        assert_eq!(
            parse_apple_music_metadata("爱在西元前".into(), "周杰伦 — 范特西".into(), None,),
            crate::domain::TrackIdentity {
                title: "爱在西元前".into(),
                artist: "周杰伦".into(),
                album: Some("范特西".into()),
            }
        );
    }

    #[test]
    fn separates_fixed_three_part_title() {
        assert_eq!(
            parse_apple_music_metadata(
                "爱在西元前 — 周杰伦 — 范特西".into(),
                "周杰伦 — 范特西".into(),
                None,
            ),
            crate::domain::TrackIdentity {
                title: "爱在西元前".into(),
                artist: "周杰伦".into(),
                album: Some("范特西".into()),
            }
        );
    }

    #[test]
    fn removes_matching_album_suffix_from_artist() {
        assert_eq!(
            clean_artist(
                "Taylor Swift — The Life of a Showgirl".into(),
                Some("The Life of a Showgirl")
            ),
            "Taylor Swift"
        );
    }

    #[test]
    fn preserves_artist_when_suffix_is_not_album() {
        assert_eq!(
            clean_artist("Simon & Garfunkel - Live".into(), Some("Greatest Hits")),
            "Simon & Garfunkel - Live"
        );
    }

    #[test]
    fn preserves_cjk_character_in_artist_and_title() {
        assert_eq!(
            parse_apple_music_metadata("一生一世".into(), "李一桐".into(), Some("专辑".into())),
            crate::domain::TrackIdentity {
                title: "一生一世".into(),
                artist: "李一桐".into(),
                album: Some("专辑".into())
            }
        );
    }

    #[test]
    fn removes_album_suffix_with_en_dash() {
        assert_eq!(
            clean_artist("Artist – Album".into(), Some("Album")),
            "Artist"
        );
    }

    #[test]
    fn stale_timeline_is_extrapolated_without_periodic_rewind_and_seek_still_works() {
        let sample = super::TimelineSample {
            position_ms: 10000,
            duration_ms: 200000,
            updated_at: 100000000,
            rate: 1.0,
        };
        assert_eq!(
            super::estimate_position(sample, true, 110000000).position_ms,
            11000
        );
        assert_eq!(
            super::estimate_position(sample, true, 120000000).position_ms,
            12000
        );
        assert_eq!(
            super::estimate_position(sample, false, 120000000).position_ms,
            10000
        );
        let seek = super::TimelineSample {
            position_ms: 3000,
            updated_at: 120000000,
            ..sample
        };
        assert_eq!(
            super::estimate_position(seek, true, 120000000).position_ms,
            3000
        );
        let faster = super::TimelineSample {
            rate: 2.0,
            ..sample
        };
        assert_eq!(
            super::estimate_position(faster, true, 110000000).position_ms,
            12000
        );
    }
}
