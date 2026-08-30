use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter};

use crate::domain::{LyricsLoadState, LyricsStatusPayload, TrackIdentity};
use crate::lyrics::cache;
use crate::lyrics::qq_music;

static LYRICS_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(0);
static LAST_REQUESTED_TRACK: Mutex<Option<TrackIdentity>> = Mutex::new(None);

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

        // 推送给前端
        if let Err(e) = app_handle.emit("lyrics-loaded", &document) {
            eprintln!("[Lyrics] 发送歌词失败: {}", e);
        } else {
            emit_status(&app_handle, &document.track, LyricsLoadState::Loaded, None);
            println!("[Lyrics] {:?} 歌词已发送至前端", document.source);
        }
    });
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
