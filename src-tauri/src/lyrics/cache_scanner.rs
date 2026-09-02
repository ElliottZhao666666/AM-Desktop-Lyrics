use std::path::{Path, PathBuf};
use serde_json::Value;

const FILE_PREFIX: &str = "ttmlLyrics[";
const MATCH_TOLERANCE_MS: u64 = 4_000;
const SCAN_ATTEMPTS: usize = 8;
const SCAN_INTERVAL_MS: u64 = 240;

/// 从 Apple Music WebView2 缓存中按新旧顺序寻找与当前歌曲时长匹配的真实 track id。
pub fn find_target_cache(target_duration_ms: u64) -> Result<Option<String>, String> {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else { return Ok(None) };
    let root = PathBuf::from(local_app_data)
        .join("Packages/AppleInc.AppleMusicWin_nzyj5cx40ttqa/AC/INetCache");
    let mut files = Vec::new();
    println!("[CacheScanner] 开始扫描: root={}, target_duration_ms={}, tolerance_ms={}, attempts={}, interval_ms={}", root.display(), target_duration_ms, MATCH_TOLERANCE_MS, SCAN_ATTEMPTS, SCAN_INTERVAL_MS);
    for attempt in 0..SCAN_ATTEMPTS {
        files.clear();
        collect_files(&root, &mut files)?;
        files.sort_by_key(|path| std::fs::metadata(path).and_then(|m| m.modified()).ok());
        files.reverse();
        println!("[CacheScanner] 第 {}/{} 次扫描，找到 {} 个候选文件", attempt + 1, SCAN_ATTEMPTS, files.len());

        for path in &files {
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let Ok(json) = serde_json::from_str::<Value>(&content) else { continue };
        let Some(ttml) = find_string(&json, "ttml") else { continue };
        let Some(duration_ms) = body_duration_ms(&ttml) else { continue };
        println!("[CacheScanner] 检查: path={}, duration_ms={}", path.display(), duration_ms);
        if duration_ms.abs_diff(target_duration_ms) <= MATCH_TOLERANCE_MS {
            if let Some(id) = find_string(&json, "lyricsId") {
                let id = id.strip_prefix("AP_").unwrap_or(&id).to_string();
                if !id.is_empty() {
                    println!("[CacheScanner] 命中: track_id={}, path={}", id, path.display());
                    return Ok(Some(id));
                }
            }
        }
        }
        if attempt + 1 < SCAN_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(SCAN_INTERVAL_MS));
        }
    }
    println!("[CacheScanner] 扫描结束，未找到匹配的 lyricsId");
    Ok(None)
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.is_dir() { return Ok(()) }
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() { collect_files(&path, files)?; }
        else if path.file_name().and_then(|n| n.to_str()).is_some_and(is_cache_name) { files.push(path); }
    }
    Ok(())
}

fn is_cache_name(name: &str) -> bool {
    name.starts_with(FILE_PREFIX) && name.ends_with("].json")
}

fn body_duration_ms(ttml: &str) -> Option<u64> {
    let start = ttml.find("<body")?;
    let end = ttml[start..].find('>')? + start;
    let body = &ttml[start..end];
    let value = body.split("dur=").nth(1)?.trim_start_matches(['"', '\'']);
    let value = value.split(['"', '\'']).next()?;
    let value = value.strip_suffix('s').unwrap_or(value);
    let parts: Vec<_> = value.split(':').collect();
    let seconds = match parts.as_slice() {
        [seconds] => seconds.parse::<f64>().ok()?,
        [minutes, seconds] => minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?,
        [hours, minutes, seconds] => hours.parse::<f64>().ok()? * 3600.0 + minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?,
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then_some((seconds * 1000.0).round() as u64)
}

fn find_string(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => map.get(key).and_then(Value::as_str).map(str::to_owned)
            .or_else(|| map.values().find_map(|v| find_string(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_string(v, key)),
        _ => None,
    }
}