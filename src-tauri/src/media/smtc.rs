// =============================================================================
// SMTC (System Media Transport Controls) 后端实现
// 用于从 Windows 系统媒体控制中读取 Apple Music 的播放状态，并通过 Tauri 事件
// 将媒体信息（标题、艺术家、播放状态、进度）推送给前端。
// =============================================================================

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{command, Emitter};
use windows::core::Interface;

// ---- Windows API 导入 ----
// 从 windows crate 导入 SMTC 相关的 WinRT 类型
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
// TypedEventHandler 用于注册 WinRT 事件回调
use crate::domain::{
    MediaPositionPayload, MediaSnapshotPayload, MediaStatePayload, PlaybackState, TrackIdentity,
};
use windows::Foundation::TypedEventHandler;
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

// =============================================================================
// 全局会话存储
// 使用 Mutex<Option<...>> 保存当前活跃的 SMTC 会话引用，
// 供 Tauri 命令（play_pause / next_track / prev_track）同步访问。
// =============================================================================
static CURRENT_SESSION: Mutex<Option<GlobalSystemMediaTransportControlsSession>> = Mutex::new(None);

static CURRENT_MEDIA_STATE: Mutex<Option<MediaStatePayload>> = Mutex::new(None);
static CURRENT_MEDIA_POSITION: Mutex<MediaPositionPayload> = Mutex::new(MediaPositionPayload {
    position_ms: 0,
    duration_ms: 0,
});

// =============================================================================
// 辅助函数
// =============================================================================

/// 将 Windows SMTC 的 PlaybackStatus 枚举转换为我们自定义的 PlaybackState
fn convert_status(
    status: GlobalSystemMediaTransportControlsSessionPlaybackStatus,
) -> PlaybackState {
    match status {
        // 正在播放
        GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => PlaybackState::Playing,
        // 已暂停
        GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => PlaybackState::Paused,
        // 已停止或已关闭
        GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped
        | GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed => PlaybackState::Stopped,
        // 其他状态（Opened / Changing 等）视为未知
        _ => PlaybackState::Unknown,
    }
}

/// 在所有活跃的 SMTC 会话中严格查找 Apple Music 会话。
/// 不回退到系统当前会话，避免读取或控制 Spotify、浏览器等其他播放器。
fn find_apple_music_session(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
) -> windows::core::Result<Option<GlobalSystemMediaTransportControlsSession>> {
    // GetSessions() 返回当前所有已注册 SMTC 的应用会话列表
    let sessions = manager.GetSessions()?;
    for i in 0..sessions.Size()? {
        let session = sessions.GetAt(i)?;
        // SourceAppUserModelId() 返回应用的 AppUserModelId 字符串
        let app_id = session.SourceAppUserModelId()?.to_string();
        let app_id_lower = app_id.to_lowercase();
        if app_id_lower.contains("applemusic") {
            println!("[SMTC] 找到 Apple Music 会话: {}", app_id);
            return Ok(Some(session));
        }
    }
    Ok(None)
}

fn waiting_state() -> MediaStatePayload {
    MediaStatePayload {
        track: TrackIdentity {
            title: "等待 Apple Music 播放...".to_string(),
            artist: String::new(),
            album: None,
        },
        playback_state: PlaybackState::Stopped,
    }
}

fn store_media_state(payload: &MediaStatePayload) {
    match CURRENT_MEDIA_STATE.lock() {
        Ok(mut state) => *state = Some(payload.clone()),
        Err(error) => eprintln!("[SMTC] 媒体状态锁已损坏: {}", error),
    }
}

fn store_media_position(payload: &MediaPositionPayload) {
    match CURRENT_MEDIA_POSITION.lock() {
        Ok(mut position) => *position = payload.clone(),
        Err(error) => eprintln!("[SMTC] 媒体进度锁已损坏: {}", error),
    }
}

fn emit_media_state(app_handle: &tauri::AppHandle, payload: &MediaStatePayload) {
    store_media_state(payload);
    if let Err(error) = app_handle.emit("media-state-changed", payload) {
        eprintln!("[SMTC] 发送媒体状态失败: {}", error);
    }
}

fn clear_current_session() {
    match CURRENT_SESSION.lock() {
        Ok(mut session) => *session = None,
        Err(error) => eprintln!("[SMTC] 当前会话锁已损坏: {}", error),
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn metadata_segments(value: &str) -> Vec<&str> {
    value
        .split(|ch| matches!(ch, '—' | '–' | '－' | '一'))
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
        .filter(|(_, ch)| matches!(ch, '-' | '—' | '–' | '－' | '一'))
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
    })
}

// =============================================================================
// SmtcManager —— SMTC 后台管理器
// =============================================================================

/// SmtcManager 负责在独立线程中运行 SMTC 监听逻辑。
/// 内部持有 Arc<AppHandle> 以便在回调和轮询中向前端发射事件。
pub struct SmtcManager {
    app_handle: Arc<tauri::AppHandle>,
}

impl SmtcManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle: Arc::new(app_handle),
        }
    }

    /// 启动 SMTC 监听。
    /// 使用独立操作系统线程隔离 WinRT 的同步等待，避免阻塞 Tauri 主线程。
    pub fn start(self) {
        let app_handle = self.app_handle;

        std::thread::spawn(move || {
            let _apartment = match WinRtApartment::initialize() {
                Ok(apartment) => apartment,
                Err(error) => {
                    eprintln!("[SMTC] 初始化 WinRT 失败: {:?}", error);
                    return;
                }
            };

            smtc_main_loop(app_handle);
        });
    }
}

/// SMTC 主循环（异步），负责：
/// 1. 获取系统 SMTC 会话管理器
/// 2. 查找 Apple Music 会话
/// 3. 注册媒体属性变化和播放状态变化的事件回调
/// 4. 持续轮询播放进度
/// SMTC 状态机主循环（异步），负责：
/// 1. Searching (寻找状态): 轮询查找目标会话，发送“等待”状态
/// 2. Connected (连接状态): 注册事件
/// 3. Syncing (同步状态): 每秒读取权威进度，前端在两次校准间插值
/// 4. Disconnected (断开状态): 会话失效时注销事件并退回 Searching
fn smtc_main_loop(app_handle: Arc<tauri::AppHandle>) {
    let mut current_session: Option<GlobalSystemMediaTransportControlsSession> = None;
    let mut props_token: Option<i64> = None;
    let mut playback_token: Option<i64> = None;
    let mut validation_ticks = 0_u8;

    loop {
        // Manager 在一轮生命周期内长期持有；申请失败时每秒重试。
        let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            Ok(op) => match op.get() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[SMTC] 请求 Manager 失败: {:?}", e);
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            },
            Err(e) => {
                eprintln!("[SMTC] RequestAsync 失败: {:?}", e);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        // Manager 自身失效时跳出内层循环并重新申请。
        loop {
            // 寻找状态 (Searching)
            if current_session.is_none() {
                match find_apple_music_session(&manager) {
                    Ok(Some(session)) => {
                        println!("[SMTC] 找到目标会话，进入 Connected 状态");

                        // 将会话引用存入全局变量
                        match CURRENT_SESSION.lock() {
                            Ok(mut guard) => *guard = Some(session.clone()),
                            Err(error) => {
                                eprintln!("[SMTC] 当前会话锁已损坏: {}", error);
                                std::thread::sleep(Duration::from_secs(1));
                                continue;
                            }
                        }

                        // 注册 MediaPropertiesChanged
                        let handle_for_props = app_handle.clone();
                        let props_handler = TypedEventHandler::new(
                            move |session_ref: windows::core::Ref<
                                '_,
                                GlobalSystemMediaTransportControlsSession,
                            >,
                                  _args| {
                                if let Some(s) = &*session_ref {
                                    match read_media_state(s) {
                                        Ok(payload) => {
                                            println!(
                                                "[SMTC] 媒体属性变化: {} - {} ({:?})",
                                                payload.track.title,
                                                payload.track.artist,
                                                payload.playback_state
                                            );
                                            emit_media_state(&handle_for_props, &payload);

                                            // 触发歌词搜索联动
                                            crate::lyrics::provider::fetch_and_emit_lyrics(
                                                payload.track.clone(),
                                                (*handle_for_props).clone(),
                                            );
                                        }
                                        Err(e) => eprintln!("[SMTC] 读取媒体属性失败: {:?}", e),
                                    }
                                }
                                Ok(())
                            },
                        );

                        // 注册 PlaybackInfoChanged
                        let handle_for_playback = app_handle.clone();
                        let playback_handler = TypedEventHandler::new(
                            move |session_ref: windows::core::Ref<
                                '_,
                                GlobalSystemMediaTransportControlsSession,
                            >,
                                  _args| {
                                if let Some(s) = &*session_ref {
                                    if let Ok(payload) = read_media_state(s) {
                                        println!(
                                            "[SMTC] 播放状态变化: {:?}",
                                            payload.playback_state
                                        );
                                        emit_media_state(&handle_for_playback, &payload);
                                    }
                                }
                                Ok(())
                            },
                        );

                        // 保存事件注册的 Token，用于后续注销
                        let pt = match session.MediaPropertiesChanged(&props_handler) {
                            Ok(token) => token,
                            Err(error) => {
                                eprintln!("[SMTC] 注册媒体属性监听失败: {:?}", error);
                                clear_current_session();
                                std::thread::sleep(Duration::from_secs(1));
                                continue;
                            }
                        };
                        let pbt = match session.PlaybackInfoChanged(&playback_handler) {
                            Ok(token) => token,
                            Err(error) => {
                                eprintln!("[SMTC] 注册播放状态监听失败: {:?}", error);
                                let _ = session.RemoveMediaPropertiesChanged(pt);
                                clear_current_session();
                                std::thread::sleep(Duration::from_secs(1));
                                continue;
                            }
                        };

                        // 初始触发一次状态发送和歌词检索
                        if let Ok(payload) = read_media_state(&session) {
                            emit_media_state(&app_handle, &payload);
                            crate::lyrics::provider::fetch_and_emit_lyrics(
                                payload.track.clone(),
                                (*app_handle).clone(),
                            );
                        }

                        current_session = Some(session);
                        props_token = Some(pt);
                        playback_token = Some(pbt);
                        validation_ticks = 0;
                    }
                    Ok(None) => {
                        // 未找到，发送等待状态并休眠 1000ms
                        let payload = waiting_state();
                        emit_media_state(&app_handle, &payload);
                        store_media_position(&MediaPositionPayload {
                            position_ms: 0,
                            duration_ms: 0,
                        });
                        std::thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                    Err(error) => {
                        eprintln!("[SMTC] 枚举媒体会话失败: {:?}", error);
                        break;
                    }
                }
            }

            // 同步状态 (Syncing)
            if let Some(session) = &current_session {
                let timeline_res = session.GetTimelineProperties();
                let mut is_disconnected = false;

                match timeline_res {
                    Ok(timeline) => match timeline.Position() {
                        Ok(ts) => {
                            let position_ticks = ts.Duration;
                            let duration_ticks = match (timeline.EndTime(), timeline.StartTime()) {
                                (Ok(end), Ok(start)) => end.Duration - start.Duration,
                                _ => 0,
                            };
                            let position_ms = (position_ticks / 10_000).max(0) as u64;
                            let duration_ms = (duration_ticks / 10_000).max(0) as u64;
                            let payload = MediaPositionPayload {
                                position_ms,
                                duration_ms,
                            };
                            store_media_position(&payload);
                            let _ = app_handle.emit("media-position-changed", &payload);
                        }
                        Err(e) => {
                            eprintln!("[SMTC] 读取位置失败 (可能断开): {:?}", e);
                            is_disconnected = true;
                        }
                    },
                    Err(e) => {
                        eprintln!("[SMTC] 获取时间轴属性失败 (可能断开): {:?}", e);
                        is_disconnected = true;
                    }
                }

                // 每秒核验一次 Manager 中的目标对象。Apple Music 重启后即使旧对象
                // 仍能返回最后的 Timeline，也会因为对象身份不同而进入重连流程。
                validation_ticks = validation_ticks.saturating_add(1);
                if !is_disconnected && validation_ticks >= 4 {
                    validation_ticks = 0;
                    match find_apple_music_session(&manager) {
                        Ok(Some(active)) if active.as_raw() == session.as_raw() => {}
                        Ok(_) => {
                            println!("[SMTC] Apple Music 会话已关闭或替换");
                            is_disconnected = true;
                        }
                        Err(error) => {
                            eprintln!("[SMTC] 核验媒体会话失败: {:?}", error);
                            break;
                        }
                    }
                }

                // 断开状态 (Disconnected) 逻辑处理
                if is_disconnected {
                    println!("[SMTC] 会话已断开，执行清理，回退到 Searching 状态");

                    // 优雅注销事件
                    if let Some(t) = props_token {
                        let _ = session.RemoveMediaPropertiesChanged(t);
                    }
                    if let Some(t) = playback_token {
                        let _ = session.RemovePlaybackInfoChanged(t);
                    }

                    current_session = None;
                    props_token = None;
                    playback_token = None;

                    // 清空全局引用
                    clear_current_session();
                    let waiting = waiting_state();
                    emit_media_state(&app_handle, &waiting);
                    let empty_position = MediaPositionPayload {
                        position_ms: 0,
                        duration_ms: 0,
                    };
                    store_media_position(&empty_position);
                    let _ = app_handle.emit("media-position-changed", &empty_position);
                } else {
                    // 前端使用单调时钟插值，这里只需每秒提供一次权威校准。
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }

        // Manager 失效，先清理当前会话，再回到外层循环重建 Manager。
        if let Some(session) = current_session.take() {
            if let Some(token) = props_token.take() {
                let _ = session.RemoveMediaPropertiesChanged(token);
            }
            if let Some(token) = playback_token.take() {
                let _ = session.RemovePlaybackInfoChanged(token);
            }
        }
        clear_current_session();
        std::thread::sleep(Duration::from_secs(1));
    }
}

// =============================================================================
// Tauri 命令
// 这些命令通过全局 CURRENT_SESSION 访问当前活跃的 SMTC 会话，
// 调用 WinRT 异步方法的 .get() 同步等待版本。
// =============================================================================

/// 返回后端当前快照。前端在完成事件监听注册后调用，避免启动阶段的事件丢失。
#[command]
pub fn get_media_snapshot() -> Result<MediaSnapshotPayload, String> {
    let media = CURRENT_MEDIA_STATE
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .unwrap_or_else(waiting_state);
    let position = CURRENT_MEDIA_POSITION
        .lock()
        .map_err(|error| error.to_string())?
        .clone();

    Ok(MediaSnapshotPayload { media, position })
}

/// 切换播放/暂停状态
#[command]
pub fn play_pause() -> Result<(), String> {
    let session = CURRENT_SESSION
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "没有活跃的 Apple Music 会话".to_string())?;
    // TryTogglePlayPauseAsync() 异步切换播放/暂停，.get() 同步等待结果
    let accepted = session
        .TryTogglePlayPauseAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    if !accepted {
        return Err("Apple Music 拒绝了播放/暂停操作".to_string());
    }
    println!("[SMTC] 执行: 切换播放/暂停");
    Ok(())
}

/// 切换到下一曲
#[command]
pub fn next_track() -> Result<(), String> {
    let session = CURRENT_SESSION
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "没有活跃的 Apple Music 会话".to_string())?;
    // TrySkipNextAsync() 异步切换到下一曲
    let accepted = session
        .TrySkipNextAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    if !accepted {
        return Err("Apple Music 拒绝了下一曲操作".to_string());
    }
    println!("[SMTC] 执行: 下一曲");
    Ok(())
}

/// 切换到上一曲
#[command]
pub fn prev_track() -> Result<(), String> {
    let session = CURRENT_SESSION
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "没有活跃的 Apple Music 会话".to_string())?;
    // TrySkipPreviousAsync() 异步切换到上一曲
    let accepted = session
        .TrySkipPreviousAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    if !accepted {
        return Err("Apple Music 拒绝了上一曲操作".to_string());
    }
    println!("[SMTC] 执行: 上一曲");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{clean_artist, parse_apple_music_metadata};

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
    fn removes_album_suffix_with_cjk_separator_and_spacing_difference() {
        assert_eq!(
            clean_artist("蔡健雅 一 Goodbye &Hello".into(), Some("Goodbye & Hello")),
            "蔡健雅"
        );
    }

    #[test]
    fn removes_album_suffix_with_en_dash() {
        assert_eq!(
            clean_artist("Artist – Album".into(), Some("Album")),
            "Artist"
        );
    }
}
