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
    quantum_ms: u64,
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
        sampled_at_ms: now.saturating_sub(116444736000000000).max(0) as u64 / 10000,
        discontinuity: false,
        timeline_revision: 0,
    }
}

/// 同一份 SMTC 样本只建一次锚点；暂停时冻结单调时钟，恢复不累计暂停时间。
#[derive(Default)]
struct TimelineTracker {
    sample: Option<TimelineSample>,
    track_revision: u64,
    position_ms: f64,
    observed_ms: u64,
    playing: bool,
    timeline_revision: u64,
    bounds: Option<(f64, f64)>,
    awaiting_progress: bool,
}
impl TimelineTracker {
    fn observe(
        &mut self,
        sample: TimelineSample,
        playing: bool,
        track_revision: u64,
        utc_now: i64,
        monotonic_ms: u64,
    ) -> MediaPositionPayload {
        let first = self.sample.is_none() || self.track_revision != track_revision;
        let previous = if first { None } else { self.sample };
        let elapsed = monotonic_ms.saturating_sub(self.observed_ms) as f64;
        let advance = if self.playing && !self.awaiting_progress {
            elapsed * previous.map_or(1.0, |sample| sample.rate)
        } else {
            0.0
        };
        let predicted = self.position_ms + advance;
        let projected_bounds = self
            .bounds
            .map(|(lower, upper)| (lower + advance, upper + advance));
        // 迟到旧快照不能重新成为权威位置，状态变化仍在当前锚点处理。
        let fresh = previous.is_none_or(|previous| {
            sample.updated_at <= 0
                || previous.updated_at <= 0
                || sample.updated_at >= previous.updated_at
        });
        let changed = fresh
            && previous.is_none_or(|previous| {
                previous.position_ms != sample.position_ms
                    || previous.updated_at != sample.updated_at
            });
        let mut measured = estimate_position(sample, playing, utc_now);
        // 播放器改变 Position 却复用同一时间戳时，不能把旧时间戳的年龄再叠加。
        if changed && previous.is_some_and(|previous| previous.updated_at == sample.updated_at) {
            measured.position_ms = sample.position_ms;
        }
        // LastUpdatedTime 也会随其他时间轴属性刷新；Position 未变时保留连续锚点。
        let position_changed =
            previous.is_none_or(|previous| previous.position_ms != sample.position_ms);
        // 时间戳跨度可能包含暂停时间，不能把整段跨度当作连续播放。
        // 与排除了暂停/准备时间的单调预测比较，且单纯刷新时间戳不算跳转。
        let discontinuity = !first
            && changed
            && position_changed
            && (measured.position_ms as f64 - predicted).abs() > 1500.0;
        let resumed = !first && playing && !self.playing;
        let progress = changed
            && position_changed
            && previous.is_some_and(|old| sample.position_ms > old.position_ms);
        let was_waiting = self.awaiting_progress;
        self.awaiting_progress = if first || discontinuity {
            playing && sample.position_ms == 0
        } else if self.awaiting_progress {
            !progress
        } else {
            playing && resumed && sample.position_ms == 0 && predicted == 0.0
        };
        let resume_anchor = playing && !self.awaiting_progress && was_waiting;
        let reanchor = first || discontinuity || resume_anchor;
        let half_step = sample.quantum_ms.saturating_sub(1) as f64 / 2.0;
        let observed_bounds = (
            (measured.position_ms as f64 - half_step).max(0.0),
            measured.position_ms as f64 + half_step,
        );
        if self.awaiting_progress {
            // Playing 可能早于音频真正启动。没有新的进度证据时不累计准备/缓冲时间。
            self.position_ms = if first || discontinuity {
                sample.position_ms as f64
            } else {
                predicted
            };
            self.bounds = if first || discontinuity {
                Some((self.position_ms, self.position_ms))
            } else {
                projected_bounds
            };
        } else if reanchor {
            self.position_ms = measured.position_ms as f64;
            self.bounds = Some(observed_bounds);
        } else if changed && position_changed && playing {
            // 整秒位置是一个有量化误差的观测，不能当作精确毫秒重新建锚。
            // 连续观测取交集，只有匀速预测超出可信区间时才纠偏。
            let overlap = projected_bounds.and_then(|(lower, upper)| {
                let lower = lower.max(observed_bounds.0);
                let upper = upper.min(observed_bounds.1);
                (lower <= upper).then_some((lower, upper))
            });
            let bounds = overlap.unwrap_or(observed_bounds);
            self.position_ms = predicted.clamp(bounds.0, bounds.1);
            self.bounds = Some(bounds);
        } else {
            self.position_ms = predicted;
            self.bounds = projected_bounds;
        }
        if fresh {
            self.sample = Some(sample);
        }
        self.track_revision = track_revision;
        self.observed_ms = monotonic_ms;
        self.playing = playing;
        let accepted = self.sample.unwrap_or(sample);
        if reanchor {
            self.timeline_revision += 1;
        }
        MediaPositionPayload {
            position_ms: if accepted.duration_ms > 0 {
                (self.position_ms.round() as u64).min(accepted.duration_ms)
            } else {
                self.position_ms.round() as u64
            },
            duration_ms: accepted.duration_ms,
            playback_rate: if self.awaiting_progress {
                0.0
            } else {
                accepted.rate
            },
            discontinuity: reanchor,
            timeline_revision: self.timeline_revision,
            ..measured
        }
    }
}
fn read_timeline(
    session: &GlobalSystemMediaTransportControlsSession,
) -> windows::core::Result<TimelineSample> {
    let timeline = session.GetTimelineProperties()?;
    let start = timeline.StartTime()?.Duration;
    let end = timeline.EndTime()?.Duration;
    let position_ticks = (timeline.Position()?.Duration - start).max(0) as u64;
    let rate = session
        .GetPlaybackInfo()?
        .PlaybackRate()
        .and_then(|rate| rate.Value())
        .unwrap_or(1.0);
    Ok(TimelineSample {
        position_ms: position_ticks / 10000,
        duration_ms: (end - start).max(0) as u64 / 10000,
        updated_at: timeline.LastUpdatedTime()?.UniversalTime,
        // 新版 AM 的实时样本只提供整秒值；精细时间值仍保留原有毫秒精度。
        quantum_ms: if position_ticks % 10_000_000 == 0 {
            1000
        } else {
            1
        },
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
    Timeline,
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
    timeline_token: i64,
    generation: u64,
}
impl Drop for SessionBinding {
    fn drop(&mut self) {
        let _ = self.session.RemoveMediaPropertiesChanged(self.props_token);
        let _ = self.session.RemovePlaybackInfoChanged(self.playback_token);
        let _ = self
            .session
            .RemoveTimelinePropertiesChanged(self.timeline_token);
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
    let timeline_sender = sender.clone();
    let timeline = TypedEventHandler::new(
        move |_session: windows::core::Ref<'_, GlobalSystemMediaTransportControlsSession>,
              _args| {
            let _ = timeline_sender.send(MediaEvent {
                session_generation: generation,
                kind: MediaEventKind::Timeline,
                observed_at: utc_ticks(),
            });
            Ok(())
        },
    );
    let timeline_token = match session.TimelinePropertiesChanged(&timeline) {
        Ok(token) => token,
        Err(error) => {
            let _ = session.RemoveMediaPropertiesChanged(props_token);
            let _ = session.RemovePlaybackInfoChanged(playback_token);
            return Err(error);
        }
    };
    Ok(SessionBinding {
        session,
        props_token,
        playback_token,
        timeline_token,
        generation,
    })
}

pub struct SmtcManager {
    app_handle: Arc<tauri::AppHandle>,
}

// 保留最早截止时间，连续通知不能无限推迟一次已安排的读取。
fn schedule_metadata(slot: &mut Option<(Instant, i64)>, now: Instant, observed: i64) {
    let deadline = now + Duration::from_millis(100);
    *slot = Some(match *slot {
        Some((old, stamp)) => (old.min(deadline), stamp.max(observed)),
        None => (deadline, observed),
    });
}

fn timeline_has_new_duration(sample: TimelineSample, previous: &MediaPositionPayload) -> bool {
    sample.duration_ms > 0
        && previous.duration_ms > 0
        && sample.duration_ms.abs_diff(previous.duration_ms) > 2000
        && sample.position_ms < 5000
        && previous.position_ms.saturating_add(5000) >= previous.duration_ms
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

fn suspend_transition(app: &tauri::AppHandle, snapshot: &MediaSnapshotPayload) {
    println!("[SMTC] 曲目与时间轴未同步，取消旧歌词请求并等待曲目确认");
    crate::lyrics::provider::enter_waiting_state();
    crate::playback::update_lyrics(
        app,
        snapshot.media.track_revision,
        crate::domain::LyricsStatusPayload {
            track: snapshot.media.track.clone(),
            state: crate::domain::LyricsLoadState::Loading,
            message: None,
        },
        None,
    );
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
        let mut timeline_tracker = TimelineTracker::default();
        let monotonic_origin = Instant::now();
        let mut poll_due = Instant::now();
        let mut validate_due = Instant::now();
        let mut identity_due = Instant::now();
        let mut unconfirmed_track: Option<TrackIdentity> = None;
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
                                timeline_tracker = TimelineTracker::default();
                                unconfirmed_track = None;
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
                    MediaEventKind::Timeline => {
                        poll_due = Instant::now();
                    }
                    MediaEventKind::Metadata => {
                        schedule_metadata(&mut metadata_due, Instant::now(), event.observed_at);
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
            // 元数据通知可能丢失或早于属性实际更新，不能只轮询时间轴。
            if Instant::now() >= identity_due {
                identity_due = Instant::now() + Duration::from_millis(500);
                if let (Ok(current), Ok(snapshot)) = (
                    read_media_state(&connected.session),
                    crate::playback::snapshot(),
                ) {
                    if current.track != snapshot.media.track
                        || current.playback_state != snapshot.media.playback_state
                        || unconfirmed_track.is_some()
                    {
                        metadata_due = Some((Instant::now(), 0));
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
                    (Ok(first), Ok(sample), Ok(second))
                        if first.track == second.track
                            && unconfirmed_track.as_ref() != Some(&second.track) =>
                    {
                        let previous = crate::playback::snapshot().ok();
                        let changed = previous
                            .as_ref()
                            .is_none_or(|previous| previous.media.track != second.track);
                        if !changed
                            && previous
                                .as_ref()
                                .is_some_and(|old| timeline_has_new_duration(sample, &old.position))
                        {
                            unconfirmed_track = Some(second.track.clone());
                            suspend_transition(&app, previous.as_ref().unwrap());
                            metadata_due =
                                Some((Instant::now() + Duration::from_millis(200), observed_at));
                        } else {
                            if changed {
                                println!("[SMTC] 已核对新曲目: {:?}", second.track);
                                unconfirmed_track = None;
                            }
                            if changed {
                                stale_sample = if timeline_tracker.sample == Some(sample)
                                    || (observed_at > 0
                                        && sample.updated_at.saturating_add(2_000_000)
                                            < observed_at)
                                {
                                    Some(sample)
                                } else {
                                    None
                                };
                            }
                            let media = crate::playback::update_media(
                                &app,
                                normalize_playable_state(second),
                            );
                            if is_unavailable_track(&media.track) {
                                crate::lyrics::provider::enter_waiting_state();
                            } else {
                                let position = if stale_sample == Some(sample) {
                                    MediaPositionPayload::default()
                                } else {
                                    timeline_tracker.observe(
                                        sample,
                                        media.playback_state == PlaybackState::Playing,
                                        media.track_revision,
                                        utc_ticks(),
                                        monotonic_origin.elapsed().as_millis() as u64,
                                    )
                                };
                                let duration = position.duration_ms;
                                crate::playback::update_position(
                                    &app,
                                    media.track_revision,
                                    position,
                                );
                                crate::lyrics::provider::fetch_and_emit_lyrics(
                                    media.track,
                                    (*app).clone(),
                                    duration,
                                    media.track_revision,
                                );
                            }
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
                        // 获取歌词前再次核对身份，避免时间轴先变、曲目事件后到。
                        if let Ok(current) = read_media_state(&connected.session) {
                            if current.track != snapshot.media.track {
                                metadata_due = Some((Instant::now(), 0));
                                continue;
                            }
                        } else {
                            continue;
                        }
                        if timeline_has_new_duration(sample, &snapshot.position) {
                            if unconfirmed_track.is_none() {
                                println!("[SMTC] 时间轴时长先于曲目更新，暂缓旧曲目歌词请求");
                                unconfirmed_track = Some(snapshot.media.track.clone());
                                suspend_transition(&app, &snapshot);
                            }
                            metadata_due = Some((Instant::now(), 0));
                            continue;
                        }
                        if stale_sample.is_some_and(|stale| stale != sample) {
                            stale_sample = None;
                        }
                        if stale_sample != Some(sample) {
                            let position = timeline_tracker.observe(
                                sample,
                                snapshot.media.playback_state == PlaybackState::Playing,
                                snapshot.media.track_revision,
                                utc_ticks(),
                                monotonic_origin.elapsed().as_millis() as u64,
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
    #[test]
    fn metadata_notifications_have_a_bounded_deadline() {
        let now = std::time::Instant::now();
        let mut due = None;
        for i in 0..20 {
            super::schedule_metadata(
                &mut due,
                now + std::time::Duration::from_millis(i * 20),
                i as i64,
            );
        }
        assert_eq!(due.unwrap().0, now + std::time::Duration::from_millis(100));
        assert_eq!(due.unwrap().1, 19);
    }
    #[test]
    fn new_song_timeline_is_not_paired_with_finished_song() {
        let sample = super::TimelineSample {
            position_ms: 0,
            duration_ms: 210000,
            updated_at: 10000000,
            rate: 1.0,
            quantum_ms: 1000,
        };
        let old = crate::domain::MediaPositionPayload {
            position_ms: 179000,
            duration_ms: 180000,
            ..Default::default()
        };
        assert!(super::timeline_has_new_duration(sample, &old));
        assert!(!super::timeline_has_new_duration(
            super::TimelineSample {
                duration_ms: 180000,
                ..sample
            },
            &old
        ));
        assert!(!super::timeline_has_new_duration(
            sample,
            &crate::domain::MediaPositionPayload {
                position_ms: 10000,
                ..old
            }
        ));
    }
    #[test]
    #[ignore = "用户授权时只读采样实时曲目与时间轴，不操作播放器"]
    fn trace_live_track_transitions() {
        let _apartment = super::WinRtApartment::initialize().unwrap();
        let manager = windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager::RequestAsync().unwrap().get().unwrap();
        let session = super::find_apple_music_session(&manager)
            .unwrap()
            .expect("Apple Music session");
        for _ in 0..100 {
            let media = super::read_media_state(&session).unwrap();
            let t = super::read_timeline(&session).unwrap();
            println!(
                "SAMPLE {} {:?} {} {} {:?}",
                super::utc_ticks(),
                media.track,
                t.position_ms,
                t.duration_ms,
                media.playback_state
            );
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    use super::{clean_artist, is_unavailable_track, parse_apple_music_metadata};
    #[test]
    fn unchanged_samples_use_monotonic_time_and_resume_excludes_pause_time() {
        let sample = super::TimelineSample {
            position_ms: 10000,
            duration_ms: 200000,
            updated_at: 100000000,
            rate: 1.0,
            quantum_ms: 1,
        };
        let mut tracker = super::TimelineTracker::default();
        assert_eq!(
            tracker.observe(sample, true, 1, 100000000, 0).position_ms,
            10000
        );
        assert_eq!(
            tracker
                .observe(sample, true, 1, 110000000, 1000)
                .position_ms,
            11000
        );
        // 系统 UTC 大幅变化也不会重新估算同一个样本。
        assert_eq!(
            tracker
                .observe(sample, true, 1, 900000000, 2000)
                .position_ms,
            12000
        );
        assert_eq!(
            tracker
                .observe(
                    super::TimelineSample {
                        rate: 0.0,
                        ..sample
                    },
                    false,
                    1,
                    910000000,
                    2100
                )
                .position_ms,
            12100
        );
        assert_eq!(
            tracker
                .observe(sample, false, 1, 950000000, 6000)
                .position_ms,
            12100
        );
        assert_eq!(
            tracker
                .observe(sample, true, 1, 950000000, 6000)
                .position_ms,
            12100
        );
        let waiting = tracker.observe(sample, true, 1, 960000000, 7000);
        assert_eq!(waiting.position_ms, 13100);
        assert_eq!(waiting.playback_rate, 1.0);
        let progressed = super::TimelineSample {
            position_ms: 14100,
            updated_at: 970000000,
            ..sample
        };
        assert_eq!(
            tracker
                .observe(progressed, true, 1, 970000000, 8000)
                .position_ms,
            14100
        );
    }
    #[test]
    fn quantized_observations_do_not_repeatedly_move_the_clock_anchor() {
        let mut tracker = super::TimelineTracker::default();
        let sample = super::TimelineSample {
            position_ms: 21000,
            duration_ms: 200000,
            updated_at: 100000000,
            rate: 1.0,
            quantum_ms: 1000,
        };
        let first = tracker.observe(sample, true, 1, 103760000, 0);
        assert_eq!(first.position_ms, 21376);
        let next = super::TimelineSample {
            position_ms: 22000,
            updated_at: 110240000,
            ..sample
        };
        assert_eq!(
            tracker.observe(next, true, 1, 113760000, 1000).position_ms,
            22376
        );
        let skipped = super::TimelineSample {
            position_ms: 29000,
            updated_at: 171760000,
            ..sample
        };
        let correction = tracker.observe(skipped, true, 1, 173760000, 7000);
        assert!(!correction.discontinuity);
        assert!(correction.position_ms > 28376 && correction.position_ms < 28800);
        let next = super::TimelineSample {
            position_ms: 30000,
            updated_at: 182000000,
            ..sample
        };
        let after = tracker.observe(next, true, 1, 183760000, 8000);
        assert_eq!(after.position_ms, correction.position_ms + 1000);
    }

    #[test]
    fn buffering_waits_for_progress_and_pause_resume_preserves_the_anchor() {
        let mut tracker = super::TimelineTracker::default();
        let zero = super::TimelineSample {
            position_ms: 0,
            duration_ms: 200000,
            updated_at: 100000000,
            rate: 1.0,
            quantum_ms: 1000,
        };
        assert_eq!(
            tracker.observe(zero, true, 1, 100000000, 0).playback_rate,
            0.0
        );
        assert_eq!(
            tracker.observe(zero, true, 1, 130000000, 3000).position_ms,
            0
        );
        let started = super::TimelineSample {
            position_ms: 1000,
            updated_at: 130000000,
            ..zero
        };
        let ready = tracker.observe(started, true, 1, 130000000, 3000);
        assert_eq!(ready.position_ms, 1000);
        assert_eq!(ready.playback_rate, 1.0);
        assert!(ready.discontinuity);
        let paused = tracker.observe(started, false, 1, 136000000, 3600);
        assert_eq!(paused.position_ms, 1600);
        let resumed = tracker.observe(started, true, 1, 170000000, 7000);
        assert_eq!(resumed.position_ms, 1600);
        assert_eq!(resumed.playback_rate, 1.0);
        let moving = super::TimelineSample {
            position_ms: 2000,
            updated_at: 175000000,
            ..zero
        };
        let resumed = tracker.observe(moving, true, 1, 175000000, 7500);
        assert_eq!(resumed.position_ms, 2100);
        assert_eq!(resumed.playback_rate, 1.0);
    }

    #[test]
    fn paused_timestamp_age_cannot_be_mistaken_for_a_seek_after_resume() {
        let mut tracker = super::TimelineTracker::default();
        let sample = super::TimelineSample {
            position_ms: 10000,
            duration_ms: 200000,
            updated_at: 100000000,
            rate: 1.0,
            quantum_ms: 1000,
        };
        tracker.observe(sample, true, 1, 102500000, 0);
        let progress = super::TimelineSample {
            position_ms: 11000,
            updated_at: 110240000,
            ..sample
        };
        tracker.observe(progress, true, 1, 112500000, 1000);
        assert_eq!(
            tracker
                .observe(progress, false, 1, 113500000, 1100)
                .position_ms,
            11350
        );
        tracker.observe(progress, false, 1, 152500000, 5000);
        assert_eq!(
            tracker
                .observe(progress, true, 1, 162500000, 6000)
                .position_ms,
            11350
        );
        let next = super::TimelineSample {
            position_ms: 12000,
            updated_at: 172000000,
            ..sample
        };
        let resumed = tracker.observe(next, true, 1, 172500000, 7000);
        assert!(!resumed.discontinuity);
        assert_eq!(resumed.position_ms, 12350);
    }

    #[test]
    #[ignore = "只读回放本地时间轴样本，无需 Apple Music；AMDL_TIMELINE_REPLAY_INPUT/OUTPUT"]
    fn replay_saved_timeline_samples_without_live_session() {
        let input = std::env::var_os("AMDL_TIMELINE_REPLAY_INPUT").expect("input");
        let output = std::env::var_os("AMDL_TIMELINE_REPLAY_OUTPUT").expect("output");
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        let mut tracker = super::TimelineTracker::default();
        let mut previous: Option<(u64, u64, bool, f64)> = None;
        let mut result = Vec::new();
        for mut row in rows {
            let elapsed = row["elapsed_ms"].as_u64().unwrap();
            let playing = row["playing"].as_bool().unwrap();
            let sample = super::TimelineSample {
                position_ms: row["raw_ms"].as_u64().unwrap(),
                duration_ms: 1000000,
                updated_at: row["updated_at_ticks"].as_str().unwrap().parse().unwrap(),
                rate: row["rate"].as_f64().unwrap(),
                quantum_ms: if row["raw_ms"].as_u64().unwrap() % 1000 == 0 {
                    1000
                } else {
                    1
                },
            };
            let utc_now = sample.updated_at + row["sample_age_ms"].as_i64().unwrap() * 10000;
            let payload = tracker.observe(sample, playing, 1, utc_now, elapsed);
            row["estimate_ms"] = payload.position_ms.into();
            row["rate"] = payload.playback_rate.into();
            row["discontinuity"] = payload.discontinuity.into();
            row["timeline_revision"] = payload.timeline_revision.into();
            row["correction_ms"] = previous
                .map(|(time, position, was_playing, rate)| {
                    payload.position_ms as f64
                        - position as f64
                        - if was_playing {
                            (elapsed - time) as f64 * rate
                        } else {
                            0.0
                        }
                })
                .unwrap_or(0.0)
                .into();
            previous = Some((elapsed, payload.position_ms, playing, payload.playback_rate));
            result.push(row);
        }
        std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        println!("已回放 {} 条本地样本", result.len());
    }

    #[test]
    fn fresh_seek_is_marked_but_quantization_and_late_samples_are_not() {
        let sample = super::TimelineSample {
            position_ms: 10000,
            duration_ms: 200000,
            updated_at: 100000000,
            rate: 1.0,
            quantum_ms: 1,
        };
        let mut tracker = super::TimelineTracker::default();
        tracker.observe(sample, true, 1, 100000000, 0);
        let jitter = super::TimelineSample {
            position_ms: 10800,
            updated_at: 110000000,
            ..sample
        };
        assert!(
            !tracker
                .observe(jitter, true, 1, 110000000, 1000)
                .discontinuity
        );
        let seek = super::TimelineSample {
            position_ms: 3000,
            updated_at: 120000000,
            ..sample
        };
        let payload = tracker.observe(seek, true, 1, 120000000, 2000);
        assert!(payload.discontinuity);
        assert_eq!(payload.position_ms, 3000);
        let next_poll = tracker.observe(seek, true, 1, 125000000, 2500);
        assert!(!next_poll.discontinuity);
        assert_eq!(next_poll.timeline_revision, payload.timeline_revision);
        let late = tracker.observe(jitter, true, 1, 130000000, 3000);
        assert_eq!(late.position_ms, 4000);
        assert!(!late.discontinuity);
        let next_track = tracker.observe(sample, true, 2, 100000000, 4000);
        assert_eq!(next_track.position_ms, 10000);
        assert!(next_track.discontinuity);
    }
    #[test]
    fn refreshed_timestamps_without_new_position_do_not_rewind_the_anchor() {
        let sample = super::TimelineSample {
            position_ms: 42000,
            duration_ms: 200000,
            updated_at: 100000000,
            rate: 1.0,
            quantum_ms: 1,
        };
        let mut tracker = super::TimelineTracker::default();
        assert_eq!(
            tracker.observe(sample, true, 1, 101330000, 0).position_ms,
            42133
        );
        let refreshed = super::TimelineSample {
            updated_at: 105420000,
            ..sample
        };
        let payload = tracker.observe(refreshed, true, 1, 106330000, 500);
        assert_eq!(payload.position_ms, 42633);
        assert!(!payload.discontinuity);
        let progress = super::TimelineSample {
            position_ms: 43000,
            updated_at: 110000000,
            ..sample
        };
        assert_eq!(
            tracker
                .observe(progress, true, 1, 110430000, 1000)
                .position_ms,
            43043
        );
    }
    #[test]
    #[ignore = "only reads live SMTC samples, requires Apple Music"]
    fn traces_live_timeline_without_controlling_playback() {
        let _apartment = super::WinRtApartment::initialize().unwrap();
        let manager = windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager::RequestAsync().unwrap().get().unwrap();
        let session = super::find_apple_music_session(&manager)
            .unwrap()
            .expect("需要打开 Apple Music");
        let origin = std::time::Instant::now();
        let mut tracker = super::TimelineTracker::default();
        let samples = std::env::var("AMDL_TIMELINE_SAMPLES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(12)
            .clamp(2, 2000);
        let interval = std::env::var("AMDL_TIMELINE_INTERVAL_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(500)
            .clamp(25, 1000);
        let mut trace = Vec::with_capacity(samples);
        let mut previous: Option<(super::TimelineSample, bool, u64, u64)> = None;
        let media = super::read_media_state(&session).unwrap();
        println!(
            "[时间轴采样] track={:?}, samples={samples}, interval={interval}ms",
            media.track
        );
        for index in 0..samples {
            let sample = super::read_timeline(&session).unwrap();
            let playing = session.GetPlaybackInfo().unwrap().PlaybackStatus().unwrap()
                == windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
            let now = super::utc_ticks();
            let elapsed_ms = origin.elapsed().as_millis() as u64;
            let position = tracker.observe(sample, playing, 1, now, elapsed_ms);
            let correction = previous
                .map(|(old, was_playing, old_elapsed, old_position)| {
                    position.position_ms as f64
                        - old_position as f64
                        - if was_playing {
                            (elapsed_ms - old_elapsed) as f64 * old.rate
                        } else {
                            0.0
                        }
                })
                .unwrap_or(0.0);
            let row = serde_json::json!({"elapsed_ms":elapsed_ms,"raw_ms":sample.position_ms,"updated_at_ticks":sample.updated_at.to_string(),"sample_age_ms":now.saturating_sub(sample.updated_at)/10000,"playing":playing,"rate":sample.rate,"estimate_ms":position.position_ms,"correction_ms":correction,"discontinuity":position.discontinuity,"timeline_revision":position.timeline_revision});
            if previous
                .is_none_or(|(old, old_playing, _, _)| old != sample || old_playing != playing)
                || index + 1 == samples
            {
                println!("[时间轴] {row}");
            }
            trace.push(row);
            previous = Some((sample, playing, elapsed_ms, position.position_ms));
            std::thread::sleep(std::time::Duration::from_millis(interval));
        }
        if let Some(file) = std::env::var_os("AMDL_TIMELINE_TRACE_FILE") {
            std::fs::write(file, serde_json::to_vec_pretty(&trace).unwrap()).unwrap();
            println!("[时间轴采样] 已保存 {} 条完整样本", trace.len());
        }
    }

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
            quantum_ms: 1,
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
