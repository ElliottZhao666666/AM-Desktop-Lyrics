use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter};

use crate::domain::{LyricsDocument, LyricsLoadState, LyricsStatusPayload, TrackIdentity};
use crate::lyrics::cache;
use crate::lyrics::cache_scanner;
use crate::lyrics::custom_pool;
use crate::lyrics::qq_music;
use crate::lyrics::qq_music::{QqLyricsPreview, QqSongCandidate};
use crate::lyrics::ttml_parser::parse_ttml as parse_api_ttml;

static LYRICS_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(0);
static LAST_REQUESTED_TRACK: Mutex<Option<TrackIdentity>> = Mutex::new(None);
const LOCAL_CACHE_LIMIT: usize = 50;
static LOCAL_LYRICS_CACHE: Mutex<Option<Vec<LyricsDocument>>> = Mutex::new(None);

pub fn enter_waiting_state() {
    LYRICS_REQUEST_GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut last_track) = LAST_REQUESTED_TRACK.lock() {
        *last_track = None;
    }
}

/// 搜索、下载、解析并发送歌词；旧请求返回后会因 generation 校验而丢弃。
pub fn fetch_and_emit_lyrics(track: TrackIdentity, app_handle: AppHandle, target_duration_ms: u64) {
    if let Ok(mut last_track) = LAST_REQUESTED_TRACK.lock() {
        if last_track.as_ref() == Some(&track) {
            return;
        }
        *last_track = Some(track.clone());
    }
    let generation = LYRICS_REQUEST_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let requested_at = SystemTime::now();
    let title = track.title.clone();
    let artist = track.artist.clone();
    emit_status(&app_handle, &track, LyricsLoadState::Loading, None);

    // 使用 tauri 提供的全局 tokio runtime 来派发异步任务，
    // 因为此函数可能是从 Windows COM 的 RPC 回调线程中调用的（该线程没有 tokio 上下文）
    tauri::async_runtime::spawn(async move {
        if LYRICS_REQUEST_GENERATION.load(Ordering::SeqCst) != generation {
            println!("[Lyrics] 丢弃过期搜索结果: {} - {}", title, artist);
            return;
        }

        match custom_pool::get(&track) {
            Ok(Some(document)) => {
                println!("[CustomLyrics] 命中永久自定义歌词: track={track:?}");
                if LYRICS_REQUEST_GENERATION.load(Ordering::SeqCst) == generation {
                    emit_loaded(&app_handle, &document);
                }
                return;
            }
            Ok(None) => {}
            Err(error) => eprintln!("[CustomLyrics] 读取永久自定义歌词失败，继续自动匹配: {error}"),
        }

        let app_config = crate::config::read_config().unwrap_or_default().normalize();
        let api_enabled = app_config.source_enabled("appleMusicApi");
        let cache_enabled = app_config.source_enabled("appleMusicCache");
        let qq_enabled = app_config.source_enabled("qqMusic");
        let mut channel_order = Vec::new();
        if api_enabled { channel_order.push((source_priority(&app_config, "appleMusicApi"), "appleMusicApi")); }
        if cache_enabled { channel_order.push((source_priority(&app_config, "appleMusicCache"), "appleMusicCache")); }
        if qq_enabled { channel_order.push((source_priority(&app_config, "qqMusic"), "qqMusic")); }
        channel_order.sort_by_key(|(priority, _)| *priority);
        println!("[Lyrics] 渠道配置: api={}, cache={}, qq={}", api_enabled, cache_enabled, qq_enabled);
        if let Some(document) = local_cache_get(&track, &app_config) {
            println!("[LyricsCache] 命中自身缓存: track={track:?}");
            if LYRICS_REQUEST_GENERATION.load(Ordering::SeqCst) == generation {
                emit_loaded(&app_handle, &document);
            }
            return;
        }
        println!("[Lyrics] 渠道配置: api_enabled={}, cache_enabled={}, qq_enabled={}, target_duration_ms={}", api_enabled, cache_enabled, qq_enabled, target_duration_ms);
        if !api_enabled && !cache_enabled && !qq_enabled {
            emit_status(&app_handle, &track, LyricsLoadState::NotFound, Some("已禁用所有自动歌词渠道".to_string()));
            return;
        }
        let mut document = None;
        for (priority, channel) in channel_order {
            println!("[Lyrics] 尝试渠道 {channel}, priority={priority}");
            let result = match channel {
                "appleMusicApi" => fetch_apple_music_api(&track, target_duration_ms, &app_config).await,
                "appleMusicCache" => {
                    let cache_track = track.clone();
                    match tauri::async_runtime::spawn_blocking(move || cache::fetch_lyrics(&cache_track, requested_at)).await {
                        Ok(Ok(value)) => value,
                        Ok(Err(error)) => { eprintln!("[Lyrics] Apple Music 缓存读取失败: {error}"); None }
                        Err(error) => { eprintln!("[Lyrics] 缓存读取任务失败: {error}"); None }
                    }
                }
                "qqMusic" => match qq_music::fetch_lyrics(&track).await {
                    Ok(value) => value,
                    Err(error) => { eprintln!("[Lyrics] QQ 音乐请求失败: {error}"); None }
                }
                _ => None,
            };
            if result.is_some() { document = result; break; }
        }
        let Some(document) = document else {
            emit_status(&app_handle, &track, LyricsLoadState::NotFound, Some("所有歌词源均未找到匹配歌词".to_string()));
            return;
        };
        if is_current_generation(generation) {
            local_cache_put(&document);
            emit_loaded(&app_handle, &document);
        }
    });
}

fn source_priority(config: &crate::config::AppConfig, id: &str) -> u32 {
    config.lyrics_sources.iter().find(|source| source.id == id).map(|source| source.priority).unwrap_or(u32::MAX)
}

async fn fetch_apple_music_api(
    track: &TrackIdentity,
    target_duration_ms: u64,
    config: &crate::config::AppConfig,
) -> Option<LyricsDocument> {
    if !config.source_enabled("appleMusicApi") { return None; }
    println!("[Lyrics] 优先尝试 Apple Music Web API: 先从缓存提取真实 Track ID");
    if target_duration_ms == 0 {
        println!("[Lyrics] 跳过 Apple Music Web API: SMTC 未提供有效歌曲时长");
        return None;
    }
    let track_id = match cache_scanner::find_target_cache(target_duration_ms) {
        Ok(Some(id)) => id,
        Ok(None) => { println!("[Lyrics] 未能从缓存提取真实 Track ID，跳过 Apple Music Web API"); return None; }
        Err(error) => { eprintln!("[Lyrics] 缓存扫描失败: {error}"); return None; }
    };
    println!("[Lyrics] 已提取 Track ID={track_id}，开始请求 Apple Music syllable-lyrics");
    let body = match crate::lyrics::am_api::fetch_syllable_lyrics(&track_id, config.auto_chs, config.am_api_tokens.clone()).await {
        Ok(body) => body,
        Err(error) => { eprintln!("[Lyrics] Apple Music Web API 请求失败: {error}"); return None; }
    };
    let ttml = extract_ttml(&body)?;
    let lines = parse_api_ttml(&ttml).ok()?.into_iter().map(|line| crate::domain::LyricLine {
        begin_ms: line.begin.round() as u64,
        end_ms: line.end.map(|value| value.round() as u64),
        text: line.text,
        words: line.words.unwrap_or_default().into_iter().map(|word| crate::domain::LyricWord { text: word.text, begin_ms: word.begin.round() as u64, end_ms: word.end.round() as u64 }).collect(),
    }).collect::<Vec<_>>();
    if lines.is_empty() { return None; }
    let document = LyricsDocument { track: track.clone(), source: crate::domain::LyricsSource::AppleMusicApi, lines, duration_ms: None };
    Some(document)
}

fn extract_ttml(body: &str) -> Option<String> {
    fn visit(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(text) if text.contains("<tt") && text.contains("<body") => Some(text.clone()),
            serde_json::Value::Object(map) => map.values().find_map(visit),
            serde_json::Value::Array(items) => items.iter().find_map(visit),
            _ => None,
        }
    }
    serde_json::from_str(body).ok().and_then(|value| visit(&value))
}

#[tauri::command]
pub async fn search_qq_lyrics(track: TrackIdentity) -> Result<Vec<QqSongCandidate>, String> {
    validate_track(&track)?;
    qq_music::search_songs(&track)
        .await
        .map_err(|error| format!("搜索 Q音歌曲失败: {error}"))
}

#[tauri::command]
pub async fn preview_qq_lyrics(candidate: QqSongCandidate) -> Result<QqLyricsPreview, String> {
    qq_music::preview_lyrics(candidate)
        .await
        .map_err(|error| format!("获取 Q音歌词预览失败: {error}"))?
        .ok_or_else(|| "此歌曲没有可用的时间轴歌词".to_string())
}

#[tauri::command]
pub async fn rescan_local_lyrics(app: AppHandle, track: TrackIdentity) -> Result<bool, String> {
    validate_track(&track)?;
    if !crate::config::read_config().unwrap_or_default().normalize().source_enabled("appleMusicCache") {
        return Err("Apple Music 本地缓存渠道已禁用".to_string());
    }
    let generation = begin_forced_request(&track, &app);
    let requested_at = SystemTime::now();
    let cache_track = track.clone();
    let document = tauri::async_runtime::spawn_blocking(move || {
        cache::fetch_lyrics(&cache_track, requested_at)
    })
    .await
    .map_err(|error| format!("本地缓存扫描任务失败: {error}"))?
    .map_err(|error| format!("扫描 Apple Music 本地缓存失败: {error}"))?;
    if !is_current_generation(generation) {
        return Ok(false);
    }
    if let Some(document) = document {
        local_cache_put(&document);
        emit_loaded(&app, &document);
        Ok(true)
    } else {
        emit_status(
            &app,
            &track,
            LyricsLoadState::NotFound,
            Some("Apple Music 本地缓存中没有找到当前歌曲歌词".to_string()),
        );
        Ok(false)
    }
}

#[tauri::command]
pub async fn refetch_qq_lyrics(app: AppHandle, track: TrackIdentity) -> Result<bool, String> {
    validate_track(&track)?;
    if !crate::config::read_config().unwrap_or_default().normalize().source_enabled("qqMusic") {
        return Err("QQ 音乐渠道已禁用".to_string());
    }
    let generation = begin_forced_request(&track, &app);
    let document = qq_music::fetch_lyrics(&track)
        .await
        .map_err(|error| format!("重新获取 Q音歌词失败: {error}"))?;
    if !is_current_generation(generation) {
        return Ok(false);
    }
    if let Some(document) = document {
        local_cache_put(&document);
        emit_loaded(&app, &document);
        Ok(true)
    } else {
        emit_status(
            &app,
            &track,
            LyricsLoadState::NotFound,
            Some("Q音没有找到匹配歌词".to_string()),
        );
        Ok(false)
    }
}

#[tauri::command]
pub async fn apply_manual_lyrics(
    app: AppHandle,
    track: TrackIdentity,
    candidate: QqSongCandidate,
) -> Result<(), String> {
    validate_track(&track)?;
    if !crate::config::read_config().unwrap_or_default().normalize().source_enabled("manual") {
        return Err("手动匹配渠道已禁用".to_string());
    }
    let generation = begin_forced_request(&track, &app);
    let document = qq_music::fetch_lyrics_by_song(&track, &candidate)
        .await
        .map_err(|error| format!("获取选定歌词失败: {error}"))?
        .ok_or_else(|| "选定歌曲没有可用的时间轴歌词".to_string())?;
    if !is_current_generation(generation) {
        return Err("歌词请求已过期，请重新应用当前曲目".to_string());
    }
    local_cache_put(&document);
    emit_loaded(&app, &document);
    println!("[ManualLyrics] 已应用并覆盖普通缓存: track={track:?}");
    Ok(())
}

#[tauri::command]
pub async fn apply_custom_lyrics(
    app: AppHandle,
    track: TrackIdentity,
    candidate: QqSongCandidate,
) -> Result<(), String> {
    validate_track(&track)?;
    let generation = begin_forced_request(&track, &app);
    let document = qq_music::fetch_lyrics_by_song(&track, &candidate)
        .await
        .map_err(|error| format!("获取选定歌词失败: {error}"))?
        .ok_or_else(|| "选定歌曲没有可用的时间轴歌词".to_string())?;
    if !is_current_generation(generation) {
        return Err("歌词请求已过期，请重新应用当前曲目".to_string());
    }
    custom_pool::save(&track, candidate, document.clone())?;
    local_cache_put(&document);
    emit_loaded(&app, &document);
    println!("[CustomLyrics] 已保存永久绑定: track={track:?}");
    Ok(())
}

fn begin_forced_request(track: &TrackIdentity, app: &AppHandle) -> u64 {
    let generation = LYRICS_REQUEST_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut last_track) = LAST_REQUESTED_TRACK.lock() {
        *last_track = Some(track.clone());
    }
    emit_status(app, track, LyricsLoadState::Loading, None);
    generation
}

fn is_current_generation(generation: u64) -> bool {
    LYRICS_REQUEST_GENERATION.load(Ordering::SeqCst) == generation
}

fn validate_track(track: &TrackIdentity) -> Result<(), String> {
    if track.title.trim().is_empty()
        || track.artist.trim().is_empty()
        || track.title.contains("正在连接")
        || track.title.starts_with("等待 ")
    {
        Err("当前没有可匹配的播放曲目".to_string())
    } else {
        Ok(())
    }
}

fn emit_loaded(app_handle: &AppHandle, document: &LyricsDocument) {
    // 推送给前端
    if let Err(e) = app_handle.emit("lyrics-loaded", document) {
        eprintln!("[Lyrics] 发送歌词失败: {e}");
    } else {
        emit_status(app_handle, &document.track, LyricsLoadState::Loaded, None);
        println!("[Lyrics] {:?} 歌词已发送至前端", document.source);
    }
}

fn local_cache_path() -> Option<std::path::PathBuf> {
    crate::app_paths::lyrics_cache_file().ok()
}

fn local_cache_limit() -> usize {
    let Ok(path) = crate::app_paths::settings_file() else {
        return 10;
    };
    let Ok(encoded) = std::fs::read_to_string(path) else {
        return 10;
    };
    let Ok(bytes) =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded.trim())
    else {
        return 10;
    };
    serde_json::from_slice::<serde_json::Value>(&bytes)
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
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn local_cache_get(track: &TrackIdentity, config: &crate::config::AppConfig) -> Option<LyricsDocument> {
    let mut cache = LOCAL_LYRICS_CACHE.lock().ok()?;
    let entries = cache.get_or_insert_with(load_local_cache);
    let index = entries.iter().position(|entry| {
        entry.track == *track && match entry.source {
            crate::domain::LyricsSource::AppleMusicApi => config.source_enabled("appleMusicApi"),
            crate::domain::LyricsSource::AppleMusicCache => config.source_enabled("appleMusicCache"),
            crate::domain::LyricsSource::QqMusic => config.source_enabled("qqMusic"),
            _ => true,
        }
    })?;
    let document = entries.remove(index);
    entries.insert(0, document.clone());
    persist_local_cache(entries);
    Some(document)
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
    match serde_json::to_string(entries) {
        Ok(text) => {
            if let Err(error) = std::fs::write(path, text) {
                eprintln!("[LyricsCache] 写入自身缓存失败: {error}");
            }
        }
        Err(error) => eprintln!("[LyricsCache] 序列化自身缓存失败: {error}"),
    }
}

fn emit_status(
    app_handle: &AppHandle,
    track: &TrackIdentity,
    state: LyricsLoadState,
    message: Option<String>,
) {
    let payload = LyricsStatusPayload {
        track: track.clone(),
        state,
        message,
    };
    if let Err(error) = app_handle.emit("lyrics-status-changed", payload) {
        eprintln!("[Lyrics] 发送状态失败: {error}");
    }
}
