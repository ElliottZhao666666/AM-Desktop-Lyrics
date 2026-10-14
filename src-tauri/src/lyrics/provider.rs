use crate::domain::{LyricsDocument, LyricsLoadState, LyricsStatusPayload, TrackIdentity};
use crate::lyrics::qq_music::{QqLyricsPreview, QqSongCandidate};
use crate::lyrics::{cache, custom_pool, qq_music};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RequestToken {
    generation: u64,
    track_revision: u64,
}
struct RequestState {
    generation: u64,
    track: Option<TrackIdentity>,
    track_revision: u64,
    duration_ms: u64,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    retry_at: Option<Instant>,
}
static REQUESTS: Mutex<RequestState> = Mutex::new(RequestState {
    generation: 0,
    track: None,
    track_revision: 0,
    duration_ms: 0,
    task: None,
    retry_at: None,
});
const LOCAL_CACHE_LIMIT: usize = 50;
static LOCAL_LYRICS_CACHE: Mutex<Option<Vec<LyricsDocument>>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LyricsRefetchSource {
    All,
    AppleMusicApi,
    AppleMusicCache,
    QqMusic,
}

impl LyricsRefetchSource {
    fn includes_pools(self) -> bool {
        self == Self::All
    }
    fn channel_id(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::AppleMusicApi => Some("appleMusicApi"),
            Self::AppleMusicCache => Some("appleMusicCache"),
            Self::QqMusic => Some("qqMusic"),
        }
    }
}

fn fetch_channels(config: &crate::config::AppConfig, source: LyricsRefetchSource) -> Vec<&str> {
    // 用户主动点选渠道仅影响本次请求，即使该渠道未启用，也不改写配置。
    if let Some(id) = source.channel_id() {
        return vec![id];
    }
    let mut channels: Vec<_> = config
        .lyrics_sources
        .iter()
        .filter(|source| {
            source.enabled
                && matches!(
                    source.id.as_str(),
                    "appleMusicApi" | "appleMusicCache" | "qqMusic"
                )
        })
        .collect();
    channels.sort_by_key(|source| source.priority);
    channels
        .into_iter()
        .map(|source| source.id.as_str())
        .collect()
}

pub fn enter_waiting_state() {
    if let Ok(mut state) = REQUESTS.lock() {
        state.generation += 1;
        state.track = None;
        state.retry_at = None;
        if let Some(task) = state.task.take() {
            task.abort();
        }
    }
}

fn begin_request(
    track: &TrackIdentity,
    track_revision: u64,
    duration_ms: u64,
    force: bool,
) -> Option<RequestToken> {
    let mut state = REQUESTS.lock().ok()?;
    let snapshot = crate::playback::snapshot().ok()?;
    if snapshot.media.track_revision != track_revision || snapshot.media.track != *track {
        return None;
    }
    let same = state.track.as_ref() == Some(track) && state.track_revision == track_revision;
    let improved_duration = (state.duration_ms == 0 && duration_ms > 0)
        || (state.duration_ms > 0
            && duration_ms > 0
            && state.duration_ms.abs_diff(duration_ms) > 1000);
    if !force
        && same
        && !improved_duration
        && state
            .retry_at
            .is_none_or(|deadline| Instant::now() < deadline)
    {
        return None;
    }
    if let Some(task) = state.task.take() {
        task.abort();
    }
    state.generation += 1;
    state.track = Some(track.clone());
    state.track_revision = track_revision;
    state.duration_ms = duration_ms;
    state.retry_at = None;
    Some(RequestToken {
        generation: state.generation,
        track_revision,
    })
}

pub fn fetch_and_emit_lyrics(
    track: TrackIdentity,
    app: AppHandle,
    duration_ms: u64,
    track_revision: u64,
) {
    start_fetch(track, app, duration_ms, track_revision, false);
}

pub fn refresh_current_lyrics(app: AppHandle) {
    if let Ok(snapshot) = crate::playback::snapshot() {
        if validate_track(&snapshot.media.track).is_ok() {
            start_fetch(
                snapshot.media.track,
                app,
                snapshot.position.duration_ms,
                snapshot.media.track_revision,
                true,
            );
        }
    }
}

fn start_fetch(
    track: TrackIdentity,
    app: AppHandle,
    duration_ms: u64,
    track_revision: u64,
    force: bool,
) {
    let Some(token) = begin_request(&track, track_revision, duration_ms, force) else {
        return;
    };
    println!(
        "[歌词][请求{}] 开始匹配: track={track:?}, duration={duration_ms}ms, 强制刷新={force}",
        token.generation
    );
    publish_status(&app, &track, LyricsLoadState::Loading, None, token);
    let task = tauri::async_runtime::spawn(async move {
        let result = tokio::time::timeout(
            Duration::from_secs(90),
            fetch_document(
                &track,
                duration_ms,
                SystemTime::now(),
                !force,
                token.generation,
                LyricsRefetchSource::All,
            ),
        )
        .await
        .unwrap_or_else(|_| Err("歌词获取超时，请检查网络或手动重试".into()));
        match result {
            Ok(Some(document)) => {
                finish_loaded(&app, &document, token);
            }
            Ok(None) => publish_status(
                &app,
                &track,
                LyricsLoadState::NotFound,
                Some("所有歌词源均未找到可信匹配歌词".into()),
                token,
            ),
            Err(error) => publish_status(&app, &track, LyricsLoadState::Error, Some(error), token),
        }
    });
    if let Ok(mut state) = REQUESTS.lock() {
        if state.generation == token.generation {
            state.task = Some(task);
        } else {
            task.abort();
        }
    }
}

async fn fetch_document(
    track: &TrackIdentity,
    duration_ms: u64,
    requested_at: SystemTime,
    use_automatic_cache: bool,
    request_id: u64,
    source: LyricsRefetchSource,
) -> Result<Option<LyricsDocument>, String> {
    let config = crate::config::read_config()?.normalize();
    if source.includes_pools() {
        if config.source_enabled("manual") {
            match custom_pool::get(track) {
                Ok(Some(document)) => {
                    println!("[歌词][请求{request_id}] 命中永久手动匹配，使用自选歌词池");
                    return Ok(Some(document));
                }
                Ok(None) => {}
                Err(error) => eprintln!("[CustomLyrics] 读取失败，继续自动匹配: {error}"),
            }
        }
        if let Some(document) = local_cache_get(track, &config, duration_ms) {
            // 保存渠道/Token 后重新按优先级获取；手动选择仍作为用户明确的覆盖结果。
            if use_automatic_cache || document.source == crate::domain::LyricsSource::Manual {
                println!(
                    "[歌词][请求{request_id}] 命中 AMDL 自身缓存，原始来源={}",
                    source_name(&document.source)
                );
                return Ok(Some(document));
            }
        }
    }
    let channels = fetch_channels(&config, source);
    let mut verified_cache: Option<Option<cache::VerifiedCache>> = None;
    let mut errors = Vec::new();
    for channel in channels {
        let name = config
            .lyrics_sources
            .iter()
            .find(|item| item.id == channel)
            .map(|item| item.name.as_str())
            .unwrap_or(channel);
        let next_step = if source == LyricsRefetchSource::All {
            "继续下一渠道"
        } else {
            "本次仅尝试此渠道"
        };
        println!("[歌词][请求{request_id}] 尝试渠道: {name}");
        let result = match channel {
            "appleMusicApi" | "appleMusicCache" => {
                if verified_cache.is_none() {
                    verified_cache = Some(
                        match cache::fetch_lyrics(track, requested_at, duration_ms, &config).await {
                            Ok(candidate) => candidate,
                            Err(error) => {
                                eprintln!("[歌词][请求{request_id}] Apple 缓存身份核验失败，跳过依赖此 ID 的渠道: {}", brief_error(&error));
                                errors.push(error);
                                None
                            }
                        },
                    );
                }
                match verified_cache.as_ref().and_then(Option::as_ref) {
                    Some(candidate) if channel == "appleMusicApi" => {
                        fetch_apple_music_api(track, candidate, &config).await
                    }
                    Some(candidate) => Ok(Some(candidate.document.clone())),
                    None => Ok(None),
                }
            }
            "qqMusic" => qq_music::fetch_matched_lyrics(track, duration_ms)
                .await
                .map_err(|error| format!("QQ 音乐获取失败: {error}")),
            _ => Ok(None),
        };
        match result {
            Ok(Some(document)) => return Ok(Some(document)),
            Ok(None) => println!("[歌词][请求{request_id}] {name} 未取得可信歌词，{next_step}"),
            Err(error) => {
                eprintln!(
                    "[歌词][请求{request_id}] {name} 获取失败，{next_step}: {}",
                    brief_error(&error)
                );
                errors.push(error);
            }
        }
    }
    if errors.is_empty() {
        Ok(None)
    } else {
        Err(errors.join("；"))
    }
}

async fn fetch_apple_music_api(
    track: &TrackIdentity,
    candidate: &cache::VerifiedCache,
    config: &crate::config::AppConfig,
) -> Result<Option<LyricsDocument>, String> {
    println!(
        "[歌词][Apple Music API] 使用已核验歌曲 id={} 请求逐字歌词",
        candidate.track_id
    );
    let body = crate::lyrics::am_api::fetch_syllable_lyrics(
        &candidate.track_id,
        config.auto_chs,
        config.am_api_tokens.clone(),
    )
    .await
    .map_err(|error| error.to_string())?;
    let Some(ttml) = extract_ttml(&body) else {
        return Ok(None);
    };
    if crate::lyrics::cache_scanner::body_duration_ms(&ttml)
        .zip(candidate.document.duration_ms)
        .is_some_and(|(api_duration, cache_duration)| {
            api_duration.abs_diff(cache_duration) > crate::lyrics::cache_scanner::MATCH_TOLERANCE_MS
        })
    {
        return Err("逐字歌词响应与已核验歌曲时长不符".into());
    }
    let mut document = crate::lyrics::ttml::parse_ttml_with_translation(
        track.clone(),
        crate::domain::LyricsSource::AppleMusicApi,
        &ttml,
        config.auto_chs,
    )
    .map_err(|error| error.to_string())?;
    if document.lines.is_empty() {
        return Ok(None);
    }
    document.duration_ms = candidate.document.duration_ms.or(document.duration_ms);
    document.storage.apple_music_id = Some(candidate.track_id.clone());
    Ok(Some(document))
}

fn source_name(source: &crate::domain::LyricsSource) -> &'static str {
    use crate::domain::LyricsSource::*;
    match source {
        AppleMusicApi => "Apple Music 官方逐字歌词",
        AppleMusicCache => "Apple Music 本地缓存",
        QqMusic => "QQ 音乐",
        Manual => "手动匹配",
        AmllDb => "AMLL 歌词库",
        Dummy => "测试歌词",
    }
}

fn brief_error(error: &str) -> String {
    let mut text: String = error.chars().take(180).collect();
    if error.chars().count() > 180 {
        text.push('…');
    }
    text.replace(['\r', '\n'], " ")
}

fn extract_ttml(body: &str) -> Option<String> {
    fn visit(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(text) if text.contains("<tt") && text.contains("<body") => {
                Some(text.clone())
            }
            serde_json::Value::Object(map) => map.values().find_map(visit),
            serde_json::Value::Array(items) => items.iter().find_map(visit),
            _ => None,
        }
    }
    serde_json::from_str(body)
        .ok()
        .and_then(|value| visit(&value))
}

fn publish_status(
    app: &AppHandle,
    track: &TrackIdentity,
    status: LyricsLoadState,
    message: Option<String>,
    token: RequestToken,
) {
    let Ok(mut state) = REQUESTS.lock() else {
        return;
    };
    if state.generation != token.generation {
        return;
    }
    if matches!(status, LyricsLoadState::NotFound | LyricsLoadState::Error) {
        state.retry_at = Some(Instant::now() + Duration::from_secs(15));
    }
    let terminal = matches!(status, LyricsLoadState::NotFound | LyricsLoadState::Error);
    if terminal {
        println!(
            "[歌词][请求{}] 匹配结束: {:?}，{}",
            token.generation,
            status,
            message.as_deref().map(brief_error).unwrap_or_default()
        );
    }
    crate::playback::update_lyrics(
        app,
        token.track_revision,
        LyricsStatusPayload {
            track: track.clone(),
            state: status,
            message,
        },
        None,
    );
}

fn finish_loaded(app: &AppHandle, document: &LyricsDocument, token: RequestToken) -> bool {
    let Ok(mut state) = REQUESTS.lock() else {
        return false;
    };
    if state.generation != token.generation {
        return false;
    }
    let displayed = cache::convert_to_simplified(
        document.clone(),
        crate::config::read_config()
            .map(|config| config.auto_chs)
            .unwrap_or(false),
    );
    if crate::playback::update_lyrics(
        app,
        token.track_revision,
        LyricsStatusPayload {
            track: document.track.clone(),
            state: LyricsLoadState::Loaded,
            message: None,
        },
        Some(displayed),
    ) {
        state.retry_at = None;
        println!(
            "[歌词][请求{}] 已采用来源={}，{} 行，时间轴={}，track={:?}",
            token.generation,
            source_name(&document.source),
            document.lines.len(),
            if document.lines.iter().any(|line| !line.words.is_empty()) {
                "逐字"
            } else {
                "逐行"
            },
            document.track
        );
        local_cache_put(document);
        let _ = app.emit("lyrics-pools-changed", ());
        true
    } else {
        false
    }
}

fn begin_forced_request(
    track: &TrackIdentity,
    app: &AppHandle,
) -> Result<(RequestToken, u64), String> {
    validate_track(track)?;
    let snapshot = crate::playback::snapshot()?;
    if snapshot.media.track != *track {
        return Err("曲目已改变，请重新匹配当前曲目".into());
    }
    let token = begin_request(
        track,
        snapshot.media.track_revision,
        snapshot.position.duration_ms,
        true,
    )
    .ok_or("歌词请求锁已损坏")?;
    println!(
        "[歌词][请求{}] 开始手动获取: track={track:?}",
        token.generation
    );
    publish_status(app, track, LyricsLoadState::Loading, None, token);
    Ok((token, snapshot.position.duration_ms))
}

fn finish_forced(
    app: &AppHandle,
    track: &TrackIdentity,
    token: RequestToken,
    result: Result<Option<LyricsDocument>, String>,
) -> Result<bool, String> {
    match result {
        Ok(Some(document)) => Ok(finish_loaded(app, &document, token)),
        Ok(None) => {
            publish_status(
                app,
                track,
                LyricsLoadState::NotFound,
                Some("未找到可信匹配歌词".into()),
                token,
            );
            Ok(false)
        }
        Err(error) => {
            publish_status(
                app,
                track,
                LyricsLoadState::Error,
                Some(error.clone()),
                token,
            );
            Err(error)
        }
    }
}

fn validate_track(track: &TrackIdentity) -> Result<(), String> {
    if track.title.trim().is_empty()
        || track.artist.trim().is_empty()
        || track.title.contains("正在连接")
        || track.title.starts_with("等待")
    {
        Err("当前没有可匹配的播放曲目".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn search_qq_lyrics(track: TrackIdentity) -> Result<Vec<QqSongCandidate>, String> {
    if track.title.trim().is_empty() {
        return Err("请输入歌曲名".into());
    }
    qq_music::search_songs(&track)
        .await
        .map_err(|error| format!("搜索 Q音歌曲失败: {error}"))
}
#[tauri::command]
pub async fn preview_qq_lyrics(candidate: QqSongCandidate) -> Result<QqLyricsPreview, String> {
    qq_music::preview_lyrics(candidate)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "此歌曲没有可用的时间轴歌词".into())
}
#[tauri::command]
pub async fn refetch_lyrics(
    app: AppHandle,
    track: TrackIdentity,
    source: LyricsRefetchSource,
) -> Result<bool, String> {
    let (token, duration) = begin_forced_request(&track, &app)?;
    println!(
        "[歌词][请求{}] 手动重新获取，范围={source:?}",
        token.generation
    );
    // 主按钮复用切歌流程，强制开启新请求，但仍允许命中两个歌词池。
    // 指定渠道时 fetch_document 跳过两个池，只走对应来源及其身份核验。
    let result = tokio::time::timeout(
        Duration::from_secs(90),
        fetch_document(
            &track,
            duration,
            SystemTime::now(),
            true,
            token.generation,
            source,
        ),
    )
    .await
    .unwrap_or_else(|_| Err("歌词获取超时，请检查网络后重试".into()));
    if REQUESTS.lock().map_err(|_| "歌词请求锁已损坏")?.generation != token.generation {
        return Err("歌词请求已被新的获取任务替代".into());
    }
    let finished = finish_forced(&app, &track, token, result);
    if let Ok(mut state) = REQUESTS.lock() {
        clear_single_source_retry(&mut state, source, token);
    }
    finished
}

fn clear_single_source_retry(
    state: &mut RequestState,
    source: LyricsRefetchSource,
    token: RequestToken,
) {
    // 单渠道请求失败后不让后台的 15 秒自动重试换成全渠道；下一次曲目信息变化仍走正常流程。
    if source != LyricsRefetchSource::All && state.generation == token.generation {
        state.retry_at = None;
    }
}
#[tauri::command]
pub async fn apply_manual_lyrics(
    app: AppHandle,
    track: TrackIdentity,
    candidate: QqSongCandidate,
) -> Result<(), String> {
    if !crate::config::read_config()?
        .normalize()
        .source_enabled("manual")
    {
        return Err("手动匹配渠道已禁用".into());
    }
    apply_selected(app, track, candidate, false).await
}
#[tauri::command]
pub async fn apply_custom_lyrics(
    app: AppHandle,
    track: TrackIdentity,
    candidate: QqSongCandidate,
) -> Result<(), String> {
    apply_selected(app, track, candidate, true).await
}
async fn apply_selected(
    app: AppHandle,
    track: TrackIdentity,
    candidate: QqSongCandidate,
    permanent: bool,
) -> Result<(), String> {
    if !crate::config::read_config()?.source_enabled("manual") {
        return Err("手动匹配渠道已禁用".into());
    }
    let (token, _) = begin_forced_request(&track, &app)?;
    let result = qq_music::fetch_lyrics_by_song(&track, &candidate)
        .await
        .map_err(|error| error.to_string());
    let result = result.map(|document| {
        document.map(|mut document| {
            document.source = crate::domain::LyricsSource::Manual;
            document
        })
    });
    let result = match result {
        Ok(Some(document)) if permanent => {
            let guard = REQUESTS.lock().map_err(|_| "歌词请求锁已损坏")?;
            if guard.generation != token.generation
                || crate::playback::snapshot()?.media.track_revision != token.track_revision
            {
                return Err("曲目已改变，请重新匹配".into());
            }
            custom_pool::save(&track, candidate, document.clone()).map(|_| Some(document))
        }
        Ok(None) => Err("选定歌曲没有可用的时间轴歌词".into()),
        other => other,
    };
    if finish_forced(&app, &track, token, result)? {
        Ok(())
    } else {
        Err("歌词请求已过期".into())
    }
}
fn local_cache_path() -> Option<std::path::PathBuf> {
    crate::app_paths::lyrics_cache_file().ok()
}

fn local_cache_limit() -> usize {
    crate::settings_store::read()
        .ok()
        .and_then(|value| {
            value
                .get("localCacheLimit")
                .and_then(serde_json::Value::as_u64)
        })
        .map(|value| value.clamp(1, 50) as usize)
        .unwrap_or(10)
}

fn load_local_cache() -> Vec<LyricsDocument> {
    let Some(path) = local_cache_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| {
            let mut entries = decode_local_cache(&text)?;
            let stored: VerifiedLyricsCache = serde_json::from_str(&text).ok()?;
            let enabled = crate::config::read_config().ok()?.auto_chs;
            if stored.auto_chs != enabled {
                entries.retain(|entry| entry.source != crate::domain::LyricsSource::AppleMusicApi);
            }
            entries.truncate(local_cache_limit());
            Some(entries)
        })
        .unwrap_or_default()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VerifiedLyricsCache {
    match_version: u8,
    #[serde(default)]
    auto_chs: bool,
    entries: Vec<LyricsDocument>,
}
fn decode_local_cache(text: &str) -> Option<Vec<LyricsDocument>> {
    let cache: VerifiedLyricsCache = serde_json::from_str(text).ok()?;
    (cache.match_version == crate::lyrics::matching::MATCH_VERSION).then_some(cache.entries)
}

fn local_cache_get(
    track: &TrackIdentity,
    config: &crate::config::AppConfig,
    duration_ms: u64,
) -> Option<LyricsDocument> {
    let mut cache = LOCAL_LYRICS_CACHE.lock().ok()?;
    let entries = cache.get_or_insert_with(load_local_cache);
    let index = entries
        .iter()
        .position(|entry| cache_entry_matches(entry, track, config, duration_ms))?;
    let document = entries.remove(index);
    entries.insert(0, document.clone());
    persist_local_cache(entries);
    Some(document)
}

fn cache_entry_matches(
    entry: &LyricsDocument,
    track: &TrackIdentity,
    config: &crate::config::AppConfig,
    duration_ms: u64,
) -> bool {
    entry.track == *track
        && (entry.source == crate::domain::LyricsSource::Manual
            || entry.duration_ms.is_none_or(|duration| {
                duration_ms == 0
                    || duration.abs_diff(duration_ms)
                        <= crate::lyrics::matching::MAX_DURATION_DIFFERENCE_MS
            }))
        && match entry.source {
            crate::domain::LyricsSource::AppleMusicApi => config.source_enabled("appleMusicApi"),
            crate::domain::LyricsSource::AppleMusicCache => {
                config.source_enabled("appleMusicCache")
            }
            crate::domain::LyricsSource::QqMusic => config.source_enabled("qqMusic"),
            crate::domain::LyricsSource::Manual => config.source_enabled("manual"),
            _ => false,
        }
}
fn local_cache_put(document: &LyricsDocument) {
    let mut document = document.clone();
    document.storage.timing = Some(
        if document.lines.iter().any(|line| !line.words.is_empty()) {
            crate::domain::LyricsTiming::Word
        } else {
            crate::domain::LyricsTiming::Line
        },
    );
    if document.storage.apple_music_id.is_none() {
        document.storage.apple_music_id =
            super::am_api::cached_song(&document.track, document.duration_ms.unwrap_or(0))
                .map(|song| song.id);
    }
    let Ok(mut cache) = LOCAL_LYRICS_CACHE.lock() else {
        return;
    };
    let entries = cache.get_or_insert_with(load_local_cache);
    entries.retain(|entry| entry.track != document.track);
    entries.insert(0, document);
    entries.truncate(local_cache_limit().min(LOCAL_CACHE_LIMIT));
    persist_local_cache(entries);
    println!("[LyricsCache] 已写入自身缓存: entries={}", entries.len());
}

pub fn apply_cache_settings(limit: usize, conversion_changed: bool) -> Result<(), String> {
    if conversion_changed {
        enter_waiting_state();
    }
    let mut cache = LOCAL_LYRICS_CACHE.lock().map_err(|_| "缓存池锁已损坏")?;
    let entries = cache.get_or_insert_with(load_local_cache);
    let previous_count = entries.len();
    let mut updated = entries.clone();
    apply_cache_policy(&mut updated, limit, conversion_changed);
    write_local_cache(&updated)?;
    *entries = updated;
    if entries.len() < previous_count {
        println!(
            "[歌词池] 已按最近使用顺序裁剪缓存：{previous_count} → {} 首",
            entries.len()
        );
    }
    Ok(())
}

pub(crate) fn pool_cache_list() -> Result<Vec<LyricsDocument>, String> {
    let mut cache = LOCAL_LYRICS_CACHE.lock().map_err(|_| "缓存池锁已损坏")?;
    Ok(cache.get_or_insert_with(load_local_cache).clone())
}

pub(crate) fn pool_cache_delete(id: &str, revision: &str) -> Result<(), String> {
    let mut cache = LOCAL_LYRICS_CACHE.lock().map_err(|_| "缓存池锁已损坏")?;
    let entries = cache.get_or_insert_with(load_local_cache);
    let index = entries
        .iter()
        .position(|entry| super::pool_management::entry_id(&entry.track) == id)
        .ok_or("该缓存已被滚动淘汰，请刷新列表")?;
    super::pool_management::check_revision(&entries[index], Some(revision))?;
    let mut updated = entries.clone();
    updated.remove(index);
    write_local_cache(&updated)?;
    *entries = updated;
    Ok(())
}

pub(crate) fn invalidate_manual_cache(track: &TrackIdentity) -> Result<(), String> {
    let mut cache = LOCAL_LYRICS_CACHE.lock().map_err(|_| "缓存池锁已损坏")?;
    let entries = cache.get_or_insert_with(load_local_cache);
    let mut updated = entries.clone();
    updated.retain(|entry| {
        entry.source != crate::domain::LyricsSource::Manual
            || !custom_pool::same_track(&entry.track, track)
    });
    write_local_cache(&updated)?;
    *entries = updated;
    Ok(())
}

fn apply_cache_policy(entries: &mut Vec<LyricsDocument>, limit: usize, conversion_changed: bool) {
    if conversion_changed {
        entries.retain(|entry| entry.source != crate::domain::LyricsSource::AppleMusicApi);
    }
    entries.truncate(limit.clamp(1, LOCAL_CACHE_LIMIT));
}
fn persist_local_cache(entries: &[LyricsDocument]) {
    if let Err(error) = write_local_cache(entries) {
        eprintln!("[LyricsCache] 写入自身缓存失败: {error}");
    }
}

fn write_local_cache(entries: &[LyricsDocument]) -> Result<(), String> {
    let path = crate::app_paths::lyrics_cache_file()?;
    let parent = path.parent().ok_or("缓存目录无效")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let text = serde_json::to_string(&VerifiedLyricsCache {
        match_version: crate::lyrics::matching::MATCH_VERSION,
        auto_chs: crate::config::read_config()
            .map(|config| config.auto_chs)
            .unwrap_or(false),
        entries: entries.to_vec(),
    })
    .map_err(|error| error.to_string())?;
    crate::settings_store::atomic_write(&path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refetch_routes_follow_priority_or_isolate_the_explicit_channel() {
        let mut config = crate::config::AppConfig::default();
        for source in &mut config.lyrics_sources {
            source.enabled = source.id == "qqMusic" || source.id == "appleMusicApi";
            source.priority = if source.id == "qqMusic" { 1 } else { 10 };
        }
        assert_eq!(
            fetch_channels(&config, LyricsRefetchSource::All),
            vec!["qqMusic", "appleMusicApi"]
        );
        assert!(LyricsRefetchSource::All.includes_pools());
        for (json, source, id) in [
            (
                "\"appleMusicApi\"",
                LyricsRefetchSource::AppleMusicApi,
                "appleMusicApi",
            ),
            (
                "\"appleMusicCache\"",
                LyricsRefetchSource::AppleMusicCache,
                "appleMusicCache",
            ),
            ("\"qqMusic\"", LyricsRefetchSource::QqMusic, "qqMusic"),
        ] {
            assert_eq!(
                serde_json::from_str::<LyricsRefetchSource>(json).unwrap(),
                source
            );
            assert!(
                !source.includes_pools(),
                "单渠道不能被手动池或软件缓存提前截获"
            );
            assert_eq!(
                fetch_channels(&config, source),
                vec![id],
                "指定渠道不受其它渠道开关与优先级影响"
            );
        }
        for source in &mut config.lyrics_sources {
            source.enabled = false;
        }
        assert!(fetch_channels(&config, LyricsRefetchSource::All).is_empty());
        assert_eq!(
            fetch_channels(&config, LyricsRefetchSource::AppleMusicCache),
            vec!["appleMusicCache"]
        );
    }

    #[test]
    fn single_channel_failure_does_not_retry_with_other_sources_or_cancel_new_requests() {
        let mut state = RequestState {
            generation: 2,
            track: None,
            track_revision: 1,
            duration_ms: 0,
            task: None,
            retry_at: Some(Instant::now()),
        };
        let token = RequestToken {
            generation: 2,
            track_revision: 1,
        };
        clear_single_source_retry(&mut state, LyricsRefetchSource::All, token);
        assert!(state.retry_at.is_some());
        clear_single_source_retry(
            &mut state,
            LyricsRefetchSource::AppleMusicApi,
            RequestToken {
                generation: 1,
                ..token
            },
        );
        assert!(state.retry_at.is_some(), "旧请求不能清理新请求的重试计划");
        clear_single_source_retry(&mut state, LyricsRefetchSource::AppleMusicApi, token);
        assert!(
            state.retry_at.is_none(),
            "指定渠道失败后不能由后台转到其它歌词源"
        );
    }

    #[test]
    #[ignore = "需要隔离进程：使用临时 LOCALAPPDATA 验证完整重新获取读取两个歌词池，不请求网络"]
    fn refetch_all_restores_manual_then_recent_cache_without_network() {
        let directory =
            std::env::temp_dir().join(format!("amdl-refetch-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::env::set_var("LOCALAPPDATA", &directory);
        let mut automatic = document(crate::domain::LyricsSource::QqMusic);
        automatic.track.title = "Cached Song".into();
        automatic.lines =
            crate::lyrics::lrc::parse_lrc("[00:01.000]缓存歌词", automatic.duration_ms);
        write_local_cache(&[automatic.clone()]).unwrap();
        let mut manual = automatic.clone();
        manual.track.title = "Permanent Song".into();
        custom_pool::managed_save(None, None, manual.clone()).unwrap();
        tauri::async_runtime::block_on(async {
            let permanent = fetch_document(
                &manual.track,
                180000,
                SystemTime::now(),
                true,
                1,
                LyricsRefetchSource::All,
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(permanent.source, crate::domain::LyricsSource::Manual);
            assert_eq!(permanent.track, manual.track);
            let cached = fetch_document(
                &automatic.track,
                180000,
                SystemTime::now(),
                true,
                2,
                LyricsRefetchSource::All,
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(cached, automatic);
        });
        std::fs::remove_dir_all(directory).unwrap();
    }
    fn document(source: crate::domain::LyricsSource) -> LyricsDocument {
        LyricsDocument {
            storage: Default::default(),
            track: TrackIdentity {
                title: "Song".into(),
                artist: "Artist".into(),
                album: Some("Album".into()),
            },
            source,
            lines: Vec::new(),
            duration_ms: Some(180000),
        }
    }
    #[test]
    fn manual_cache_is_independent_of_qq_and_respects_manual_switch() {
        let manual = document(crate::domain::LyricsSource::Manual);
        let mut config = crate::config::AppConfig::default();
        config
            .lyrics_sources
            .iter_mut()
            .find(|source| source.id == "qqMusic")
            .unwrap()
            .enabled = false;
        assert!(cache_entry_matches(&manual, &manual.track, &config, 220000));
        config
            .lyrics_sources
            .iter_mut()
            .find(|source| source.id == "manual")
            .unwrap()
            .enabled = false;
        assert!(!cache_entry_matches(
            &manual,
            &manual.track,
            &config,
            180000
        ));
        let qq = document(crate::domain::LyricsSource::QqMusic);
        assert!(!cache_entry_matches(&qq, &qq.track, &config, 180000));
    }
    #[test]
    fn conversion_invalidates_localized_api_and_limit_applies_immediately() {
        let mut entries = vec![
            document(crate::domain::LyricsSource::AppleMusicApi),
            document(crate::domain::LyricsSource::Manual),
            document(crate::domain::LyricsSource::QqMusic),
        ];
        apply_cache_policy(&mut entries, 1, true);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, crate::domain::LyricsSource::Manual);
    }
    #[test]
    fn legacy_unverified_cache_cannot_reintroduce_wrong_matches() {
        assert!(decode_local_cache("[]").is_none());
        assert_eq!(
            decode_local_cache(r#"{"match_version":2,"entries":[]}"#),
            Some(Vec::new())
        );
        assert!(decode_local_cache(r#"{"match_version":0,"entries":[]}"#).is_none());
    }

    #[test]
    fn shrinking_pool_from_thirty_to_ten_keeps_most_recent_in_order() {
        let mut entries: Vec<_> = (0..30)
            .map(|index| {
                let mut doc = document(crate::domain::LyricsSource::QqMusic);
                doc.track.title = format!("recent-{index}");
                doc
            })
            .collect();
        apply_cache_policy(&mut entries, 10, false);
        assert_eq!(entries.len(), 10);
        assert_eq!(
            entries
                .iter()
                .map(|doc| doc.track.title.clone())
                .collect::<Vec<_>>(),
            (0..10)
                .map(|index| format!("recent-{index}"))
                .collect::<Vec<_>>()
        );
    }
}
