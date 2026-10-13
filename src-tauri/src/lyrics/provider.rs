use crate::domain::{LyricsDocument, LyricsLoadState, LyricsStatusPayload, TrackIdentity};
use crate::lyrics::qq_music::{QqLyricsPreview, QqSongCandidate};
use crate::lyrics::{cache, custom_pool, qq_music};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use tauri::AppHandle;

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
    publish_status(&app, &track, LyricsLoadState::Loading, None, token);
    let task = tauri::async_runtime::spawn(async move {
        let result = tokio::time::timeout(
            Duration::from_secs(90),
            fetch_document(&track, duration_ms, SystemTime::now(), !force),
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
) -> Result<Option<LyricsDocument>, String> {
    let config = crate::config::read_config()?.normalize();
    if config.source_enabled("manual") {
        match custom_pool::get(track) {
            Ok(Some(document)) => return Ok(Some(document)),
            Ok(None) => {}
            Err(error) => eprintln!("[CustomLyrics] 读取失败，继续自动匹配: {error}"),
        }
    }
    if let Some(document) = local_cache_get(track, &config, duration_ms) {
        // 保存渠道/Token 后重新按优先级获取；手动选择仍作为用户明确的覆盖结果。
        if use_automatic_cache || document.source == crate::domain::LyricsSource::Manual {
            return Ok(Some(document));
        }
    }
    let mut channels = config
        .lyrics_sources
        .iter()
        .filter(|source| {
            source.enabled
                && matches!(
                    source.id.as_str(),
                    "appleMusicApi" | "appleMusicCache" | "qqMusic"
                )
        })
        .collect::<Vec<_>>();
    channels.sort_by_key(|source| source.priority);
    let mut verified_cache: Option<Option<cache::VerifiedCache>> = None;
    let mut errors = Vec::new();
    for source in channels {
        let result = match source.id.as_str() {
            "appleMusicApi" | "appleMusicCache" => {
                if verified_cache.is_none() {
                    verified_cache = Some(
                        match cache::fetch_lyrics(track, requested_at, duration_ms, &config).await {
                            Ok(candidate) => candidate,
                            Err(error) => {
                                errors.push(error);
                                None
                            }
                        },
                    );
                }
                match verified_cache.as_ref().and_then(Option::as_ref) {
                    Some(candidate) if source.id == "appleMusicApi" => {
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
            Ok(None) => {}
            Err(error) => {
                eprintln!("[Lyrics] {error}");
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
    Ok(Some(document))
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
        local_cache_put(document);
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
pub async fn rescan_local_lyrics(app: AppHandle, track: TrackIdentity) -> Result<bool, String> {
    let config = crate::config::read_config()?.normalize();
    if !config.source_enabled("appleMusicCache") {
        return Err("Apple Music 本地缓存渠道已禁用".into());
    }
    let (token, duration) = begin_forced_request(&track, &app)?;
    let result = cache::fetch_lyrics(&track, SystemTime::now(), duration, &config)
        .await
        .map(|candidate| candidate.map(|candidate| candidate.document));
    finish_forced(&app, &track, token, result)
}
#[tauri::command]
pub async fn refetch_qq_lyrics(app: AppHandle, track: TrackIdentity) -> Result<bool, String> {
    if !crate::config::read_config()?
        .normalize()
        .source_enabled("qqMusic")
    {
        return Err("QQ 音乐渠道已禁用".into());
    }
    let (token, duration) = begin_forced_request(&track, &app)?;
    let result = qq_music::fetch_matched_lyrics(&track, duration)
        .await
        .map_err(|error| error.to_string());
    finish_forced(&app, &track, token, result)
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
    let Ok(mut cache) = LOCAL_LYRICS_CACHE.lock() else {
        return;
    };
    let entries = cache.get_or_insert_with(load_local_cache);
    entries.retain(|entry| entry.track != document.track);
    entries.insert(0, document.clone());
    entries.truncate(local_cache_limit().min(LOCAL_CACHE_LIMIT));
    persist_local_cache(entries);
    println!("[LyricsCache] 已写入自身缓存: entries={}", entries.len());
}

pub fn apply_cache_settings(limit: usize, conversion_changed: bool) {
    if conversion_changed {
        enter_waiting_state();
    }
    let Ok(mut cache) = LOCAL_LYRICS_CACHE.lock() else {
        return;
    };
    let entries = cache.get_or_insert_with(load_local_cache);
    apply_cache_policy(entries, limit, conversion_changed);
    persist_local_cache(entries);
}

fn apply_cache_policy(entries: &mut Vec<LyricsDocument>, limit: usize, conversion_changed: bool) {
    if conversion_changed {
        entries.retain(|entry| entry.source != crate::domain::LyricsSource::AppleMusicApi);
    }
    entries.truncate(limit.clamp(1, LOCAL_CACHE_LIMIT));
}
fn persist_local_cache(entries: &[LyricsDocument]) {
    let Some(path) = local_cache_path() else {
        eprintln!("[LyricsCache] LOCALAPPDATA 不可用，无法持久化自身缓存");
        return;
    };
    let Some(parent) = path.parent() else { return };
    if let Err(error) = std::fs::create_dir_all(parent) {
        eprintln!("[LyricsCache] 创建缓存目录失败: {error}");
        return;
    }
    match serde_json::to_string(&VerifiedLyricsCache {
        match_version: crate::lyrics::matching::MATCH_VERSION,
        auto_chs: crate::config::read_config()
            .map(|config| config.auto_chs)
            .unwrap_or(false),
        entries: entries.to_vec(),
    }) {
        Ok(text) => {
            if let Err(error) = crate::settings_store::atomic_write(&path, text.as_bytes()) {
                eprintln!("[LyricsCache] 写入自身缓存失败: {error}");
            }
        }
        Err(error) => eprintln!("[LyricsCache] 序列化自身缓存失败: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(source: crate::domain::LyricsSource) -> LyricsDocument {
        LyricsDocument {
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
}
