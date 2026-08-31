use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::domain::{LyricsDocument, LyricsSource, TrackIdentity};
use crate::lyrics::ttml::parse_ttml;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};

const CACHE_FILE_PREFIX: &str = "ttmlLyrics";
const MAX_CACHE_FILE_SIZE: u64 = 8 * 1024 * 1024;
const CACHE_TIMESTAMP_TOLERANCE: Duration = Duration::from_secs(2);
const CACHE_SCAN_ATTEMPTS: usize = 5;
const CACHE_SCAN_INTERVAL: Duration = Duration::from_millis(120);
const DEFAULT_AUTO_CONVERT_TRADITIONAL_TO_SIMPLIFIED: bool = true;

#[derive(serde::Deserialize)]
struct CachedLyrics {
    ttml: String,
    status: Option<String>,
}

/// Apple Music Windows 将当前歌词放在 WebView2 的 INetCache 中。
/// 文件名没有可靠的歌曲标识，因此只接受切歌请求附近新产生的最新响应。
pub fn fetch_lyrics(
    track: &TrackIdentity,
    requested_at: SystemTime,
) -> Result<Option<LyricsDocument>, Box<dyn Error + Send + Sync>> {
    #[cfg(not(windows))]
    {
        let _ = track;
        let _ = requested_at;
        return Ok(None);
    }

    #[cfg(windows)]
    {
        let Some(root) = cache_root() else {
            eprintln!("[Cache] LOCALAPPDATA 不可用，无法定位 Apple Music 缓存");
            return Ok(None);
        };
        println!(
            "[Cache] 开始查找: track={:?}, root={}, requested_at={:?}, attempts={}, interval_ms={}, tolerance_ms={}",
            track,
            root.display(),
            requested_at,
            CACHE_SCAN_ATTEMPTS,
            CACHE_SCAN_INTERVAL.as_millis(),
            CACHE_TIMESTAMP_TOLERANCE.as_millis()
        );
        for attempt in 0..CACHE_SCAN_ATTEMPTS {
            println!("[Cache] 第 {}/{} 次扫描", attempt + 1, CACHE_SCAN_ATTEMPTS);
            if let Some(document) = scan_cache_once(&root, track, requested_at)? {
                println!("[Cache] 查找完成: 命中，共 {} 行", document.lines.len());
                return Ok(Some(convert_to_simplified(document)));
            }
            if attempt + 1 < CACHE_SCAN_ATTEMPTS {
                println!(
                    "[Cache] 本次未命中，等待 {}ms",
                    CACHE_SCAN_INTERVAL.as_millis()
                );
                std::thread::sleep(CACHE_SCAN_INTERVAL);
            }
        }
        println!("[Cache] 查找完成: 所有扫描均未命中");
        Ok(None)
    }
}

fn convert_to_simplified(mut document: LyricsDocument) -> LyricsDocument {
    if !auto_convert_traditional_to_simplified() {
        return document;
    }
    let converter = match OpenCC::from_config(BuiltinConfig::T2s) {
        Ok(converter) => converter,
        Err(error) => {
            eprintln!("[Cache] 繁转简转换器初始化失败，保留原歌词: {error}");
            return document;
        }
    };
    let mut changed = false;
    for line in &mut document.lines {
        let converted = converter.convert(&line.text);
        changed |= converted != line.text;
        line.text = converted;
        for word in &mut line.words {
            let converted = converter.convert(&word.text);
            changed |= converted != word.text;
            word.text = converted;
        }
    }
    if changed {
        println!("[Cache] 检测到繁体歌词，已转换为简体");
    } else {
        println!("[Cache] 歌词无需繁转简");
    }
    document
}

fn auto_convert_traditional_to_simplified() -> bool {
    let Ok(path) = crate::app_paths::settings_file() else {
        return DEFAULT_AUTO_CONVERT_TRADITIONAL_TO_SIMPLIFIED;
    };
    let Ok(encoded) = std::fs::read_to_string(path) else {
        return DEFAULT_AUTO_CONVERT_TRADITIONAL_TO_SIMPLIFIED;
    };
    let Ok(bytes) =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded.trim())
    else {
        return DEFAULT_AUTO_CONVERT_TRADITIONAL_TO_SIMPLIFIED;
    };
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("autoConvertTraditionalToSimplified")
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(DEFAULT_AUTO_CONVERT_TRADITIONAL_TO_SIMPLIFIED)
}

#[cfg(windows)]
fn scan_cache_once(
    root: &Path,
    track: &TrackIdentity,
    requested_at: SystemTime,
) -> Result<Option<LyricsDocument>, Box<dyn Error + Send + Sync>> {
    let mut files = Vec::new();
    collect_cache_files(root, &mut files)?;
    files.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    files.reverse();
    println!("[Cache] 找到 {} 个命名匹配的候选文件", files.len());

    for path in files {
        let Ok(metadata) = std::fs::metadata(&path) else {
            eprintln!("[Cache] 跳过: 无法读取元数据: {}", path.display());
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            eprintln!("[Cache] 跳过: 无法读取修改时间: {}", path.display());
            continue;
        };
        let age_ms = requested_at
            .duration_since(modified)
            .map(|age| age.as_millis() as i128)
            .unwrap_or_else(|error| -(error.duration().as_millis() as i128));
        println!(
            "[Cache] 检查候选: path={}, size={}, modified={:?}, relative_age_ms={}",
            path.display(),
            metadata.len(),
            modified,
            age_ms
        );
        if metadata.len() > MAX_CACHE_FILE_SIZE {
            println!("[Cache] 跳过: 文件超过 {} 字节", MAX_CACHE_FILE_SIZE);
            continue;
        }
        if modified + CACHE_TIMESTAMP_TOLERANCE < requested_at {
            println!("[Cache] 跳过: 修改时间早于请求时间容差范围");
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            eprintln!("[Cache] 跳过: 无法读取 UTF-8 文本: {}", path.display());
            continue;
        };
        let Ok(cached) = serde_json::from_str::<CachedLyrics>(&content) else {
            eprintln!("[Cache] 跳过: JSON 解析失败: {}", path.display());
            continue;
        };
        if cached.status.as_deref() == Some("error") || cached.ttml.trim().is_empty() {
            println!(
                "[Cache] 跳过: status={:?}, ttml_length={}",
                cached.status,
                cached.ttml.len()
            );
            continue;
        }
        match parse_ttml(track.clone(), LyricsSource::AppleMusicCache, &cached.ttml) {
            Ok(document) if !document.lines.is_empty() => {
                println!(
                    "[Cache] 命中: path={}, ttml_length={}, lines={}",
                    path.display(),
                    cached.ttml.len(),
                    document.lines.len()
                );
                return Ok(Some(document));
            }
            Ok(_) => println!("[Cache] 跳过: TTML 解析成功但没有歌词行"),
            Err(error) => eprintln!("[Cache] 跳过: TTML 解析失败: {error}"),
        }
    }
    Ok(None)
}

#[cfg(windows)]
fn cache_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|local| {
        PathBuf::from(local)
            .join("Packages")
            .join("AppleInc.AppleMusicWin_nzyj5cx40ttqa")
            .join("AC")
            .join("INetCache")
    })
}

#[cfg(windows)]
fn collect_cache_files(
    root: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_cache_files(&path, files)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_lyrics_cache_name)
        {
            files.push(path);
        }
    }
    Ok(())
}

fn is_lyrics_cache_name(name: &str) -> bool {
    let Some(number) = name
        .strip_prefix(CACHE_FILE_PREFIX)
        .and_then(|value| value.strip_prefix('['))
        .and_then(|value| value.strip_suffix("].json"))
    else {
        return false;
    };
    !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::convert_to_simplified;
    use crate::domain::{LyricLine, LyricWord, LyricsDocument, LyricsSource, TrackIdentity};

    #[test]
    fn converts_cached_lyrics_to_simplified_chinese() {
        let document = LyricsDocument {
            track: TrackIdentity::default(),
            source: LyricsSource::AppleMusicCache,
            duration_ms: None,
            lines: vec![LyricLine {
                begin_ms: 0,
                end_ms: Some(1_000),
                text: "這條歌陪我一直到大漢".into(),
                words: vec![LyricWord {
                    text: "這條歌".into(),
                    begin_ms: 0,
                    end_ms: 1_000,
                }],
            }],
        };

        let converted = convert_to_simplified(document);
        assert_eq!(converted.lines[0].text, "这条歌陪我一直到大汉");
        assert_eq!(converted.lines[0].words[0].text, "这条歌");
    }
}
