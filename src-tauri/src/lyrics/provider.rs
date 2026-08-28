use serde::Serialize;
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

static LYRICS_REQUEST_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 假歌词行载荷
#[derive(Debug, Clone, Serialize)]
pub struct LyricLinePayload {
    pub time: u64, // 毫秒
    pub text: String,
}

/// 接收歌曲信息，模拟获取歌词并发送给前端
pub fn fetch_and_emit_lyrics(title: String, artist: String, app_handle: AppHandle) {
    let generation = LYRICS_REQUEST_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    // 使用 tauri 提供的全局 tokio runtime 来派发异步任务，
    // 因为此函数可能是从 Windows COM 的 RPC 回调线程中调用的（该线程没有 tokio 上下文）
    tauri::async_runtime::spawn(async move {
        println!("[Lyrics] 正在搜索: {} - {}", title, artist);

        // 模拟网络延迟
        tokio::time::sleep(Duration::from_secs(1)).await;

        if LYRICS_REQUEST_GENERATION.load(Ordering::SeqCst) != generation {
            println!("[Lyrics] 丢弃过期搜索结果: {} - {}", title, artist);
            return;
        }

        // 构造假歌词数据
        let dummy_lyrics = vec![
            LyricLinePayload {
                time: 0,
                text: format!("(测试歌词) {} - {}", title, artist),
            },
            LyricLinePayload {
                time: 5000,
                text: "这里是第一句歌词".to_string(),
            },
            LyricLinePayload {
                time: 10000,
                text: "这里是第二句歌词".to_string(),
            },
            LyricLinePayload {
                time: 15000,
                text: "正在随音乐流动...".to_string(),
            },
        ];

        // 推送给前端
        if let Err(e) = app_handle.emit("lyrics-loaded", &dummy_lyrics) {
            eprintln!("[Lyrics] 发送歌词失败: {}", e);
        } else {
            println!("[Lyrics] 假歌词已发送至前端");
        }
    });
}

/// 歌词获取策略模式 Trait
pub trait LyricsProvider {
    /// 异步获取歌词
    #[allow(async_fn_in_trait)]
    async fn fetch_lyrics(&self, title: &str, artist: &str) -> Result<String, Box<dyn Error>>;
}

/// 内存读取方案
pub struct MemoryReaderProvider;

impl LyricsProvider for MemoryReaderProvider {
    async fn fetch_lyrics(&self, _title: &str, _artist: &str) -> Result<String, Box<dyn Error>> {
        println!("MemoryReaderProvider: 正在从内存读取歌词...");
        // TODO: 实现具体的内存读取逻辑
        Ok(String::from("[00:00.00] 暂无歌词 (内存方案)"))
    }
}

/// 网络请求 AMLL-DB 方案
pub struct AmllDbProvider;

impl LyricsProvider for AmllDbProvider {
    async fn fetch_lyrics(&self, _title: &str, _artist: &str) -> Result<String, Box<dyn Error>> {
        println!("AmllDbProvider: 正在从 AMLL-DB 请求歌词...");
        // TODO: 实现具体的网络请求逻辑
        Ok(String::from("[00:00.00] 暂无歌词 (AMLL-DB方案)"))
    }
}
