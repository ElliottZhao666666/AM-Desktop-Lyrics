use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter};

use crate::domain::{LyricsDocument, LyricsLoadState, LyricsStatusPayload, TrackIdentity};
use crate::lyrics::cache;
use crate::lyrics::qq_music;

static LYRICS_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(0);
static LAST_REQUESTED_TRACK: Mutex<Option<TrackIdentity>> = Mutex::new(None);
const LOCAL_CACHE_LIMIT: usize = 10;
static LOCAL_LYRICS_CACHE: Mutex<Option<Vec<LyricsDocument>>> = Mutex::new(None);

/// 搜索、下载、解析并发送歌词；旧请求返回后会因 generation 校验而丢弃。
pub fn fetch_and_emit_lyrics(track: TrackIdentity, app_handle: AppHandle) {
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

        if let Some(document) = local_cache_get(&track) {
            println!("[LyricsCache] 命中自身缓存: track={track:?}");
            emit_loaded(&app_handle, &document);
            return;
        }
        println!(
            "[Lyrics] 正在读取 Apple Music 本地缓存: {} - {}",
            title, artist
        );
        let cache_track = track.clone();
        let cache_result = match tauri::async_runtime::spawn_blocking(move || {
            cache::fetch_lyrics(&cache_track, requested_at)
        })
        .await
        {
            Ok(result) => result,
            Err(error) => Err(format!("缓存读取任务失败: {error}").into()),
        };
        let result = match cache_result {
            Ok(Some(document)) => {
                println!("[Lyrics] Apple Music 缓存歌词已找到");
                Ok(Some(document))
            }
            Ok(None) => {
                println!("[Lyrics] Apple Music 缓存未命中，转入 QQ 音乐搜索");
                qq_music::fetch_lyrics(&track).await
            }
            Err(error) => {
                eprintln!("[Lyrics] Apple Music 缓存读取失败，转入 QQ 音乐搜索: {error}");
                qq_music::fetch_lyrics(&track).await
            }
        };
        if LYRICS_REQUEST_GENERATION.load(Ordering::SeqCst) != generation {
            println!("[Lyrics] 丢弃过期搜索结果: {} - {}", title, artist);
            return;
        }
        let document = match result {
            Ok(Some(document)) => document,
            Ok(None) => {
                emit_status(
                    &app_handle,
                    &track,
                    LyricsLoadState::NotFound,
                    Some("所有歌词源均未找到匹配歌词".to_string()),
                );
                return;
            }
            Err(error) => {
                eprintln!("[Lyrics] 在线歌词请求失败: {error}");
                emit_status(
                    &app_handle,
                    &track,
                    LyricsLoadState::Error,
                    Some(error.to_string()),
                );
                return;
            }
        };

        local_cache_put(&document);
        emit_loaded(&app_handle, &document);
    });
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
    std::env::var_os("LOCALAPPDATA").map(|path| {
        std::path::PathBuf::from(path)
            .join("AM-Desktop-Lyrics")
            .join("lyrics-cache.json")
    })
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

fn local_cache_get(track: &TrackIdentity) -> Option<LyricsDocument> {
    let mut cache = LOCAL_LYRICS_CACHE.lock().ok()?;
    let entries = cache.get_or_insert_with(load_local_cache);
    let index = entries.iter().position(|entry| entry.track == *track)?;
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
    entries.truncate(LOCAL_CACHE_LIMIT);
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
