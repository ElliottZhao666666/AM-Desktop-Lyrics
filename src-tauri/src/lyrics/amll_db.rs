use reqwest::Client;
use serde_json::Value;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use crate::domain::{LyricsDocument, LyricsSource, TrackIdentity};
use crate::lyrics::ttml::parse_ttml;

const INDEX_URL: &str =
    "https://cdn.jsdelivr.net/gh/amll-dev/amll-ttml-db@main/am-lyrics/index.jsonl";
const RAW_BASE_URL: &str = "https://cdn.jsdelivr.net/gh/amll-dev/amll-ttml-db@main/am-lyrics";
const INDEX_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const INDEX_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const LYRIC_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .http1_only()
            .user_agent("AMDL/1.0.0")
            .build()
            .expect("valid HTTP client")
    })
}

pub async fn fetch_lyrics(
    track: &TrackIdentity,
) -> Result<Option<LyricsDocument>, Box<dyn Error + Send + Sync>> {
    let cache = cache_dir()?;
    fs::create_dir_all(&cache)?;
    let index_path = cache.join("am-lyrics-index.jsonl");
    let index = load_index(&index_path).await?;

    let candidate = find_candidate(&index, track)?;
    let Some(mut candidate) = candidate else {
        return Ok(None);
    };
    let mut lyric_file = candidate
        .raw_lyric_file
        .as_deref()
        .ok_or_else(|| boxed_error("AMLL-DB 索引记录缺少 rawLyricFile"))?
        .to_string();
    if !is_safe_ttml_file(&lyric_file) {
        return Err(boxed_error("AMLL-DB rawLyricFile 不是安全的 TTML 文件名"));
    }
    let mut lyric_path = cache.join(&lyric_file);
    let mut document_track = track.clone();
    if document_track.album.is_none() {
        document_track.album = candidate.album.clone();
    }

    if lyric_path.is_file() {
        match fs::read_to_string(&lyric_path)
            .map_err(Into::into)
            .and_then(|xml| parse_ttml(document_track.clone(), LyricsSource::AmllDb, &xml))
        {
            Ok(document) => return Ok(Some(document)),
            Err(error) => {
                eprintln!(
                    "[Lyrics] 缓存 TTML 无效，将重新下载 {}: {}",
                    candidate.id, error
                );
            }
        }
    }

    let lyric_url = format!("{RAW_BASE_URL}/{lyric_file}");
    let xml = match download_utf8(&lyric_url, "TTML", LYRIC_REQUEST_TIMEOUT).await {
        Ok(xml) => xml,
        Err(first_error) => {
            let refreshed_index = download_utf8(INDEX_URL, "索引", INDEX_REQUEST_TIMEOUT)
                .await
                .map_err(|refresh_error| {
                    boxed_error(format!(
                        "{first_error}; 刷新 AMLL-DB 索引失败: {refresh_error}"
                    ))
                })?;
            if !index_is_valid(&refreshed_index) {
                return Err(boxed_error(format!(
                    "{first_error}; 刷新的 AMLL-DB 索引内容无效或不完整"
                )));
            }
            atomic_write(&index_path, refreshed_index.as_bytes())?;
            let Some(refreshed_candidate) = find_candidate(&refreshed_index, track)? else {
                return Ok(None);
            };
            let refreshed_file = refreshed_candidate
                .raw_lyric_file
                .as_deref()
                .ok_or_else(|| boxed_error("刷新后的 AMLL-DB 索引记录缺少 rawLyricFile"))?
                .to_string();
            if !is_safe_ttml_file(&refreshed_file) {
                return Err(boxed_error(
                    "刷新后的 AMLL-DB rawLyricFile 不是安全的 TTML 文件名",
                ));
            }
            candidate = refreshed_candidate;
            lyric_file = refreshed_file;
            lyric_path = cache.join(&lyric_file);
            if document_track.album.is_none() {
                document_track.album = candidate.album.clone();
            }
            let refreshed_url = format!("{RAW_BASE_URL}/{lyric_file}");
            download_utf8(&refreshed_url, "TTML", LYRIC_REQUEST_TIMEOUT)
                .await
                .map_err(|retry_error| {
                    boxed_error(format!(
                        "{first_error}; 刷新索引后下载 TTML 仍失败: {retry_error}"
                    ))
                })?
        }
    };
    let document = parse_ttml(document_track, LyricsSource::AmllDb, &xml)
        .map_err(|error| boxed_error(format!("AMLL-DB TTML 解析失败: {error}")))?;
    atomic_write(&lyric_path, xml.as_bytes())?;
    Ok(Some(document))
}

async fn load_index(path: &Path) -> Result<String, Box<dyn Error + Send + Sync>> {
    let cached = fs::read_to_string(path)
        .ok()
        .filter(|value| index_is_valid(value));
    if cached.is_some() && cache_is_fresh(path, INDEX_MAX_AGE) {
        return Ok(cached.unwrap());
    }

    match download_utf8(INDEX_URL, "索引", INDEX_REQUEST_TIMEOUT).await {
        Ok(index) if index_is_valid(&index) => {
            atomic_write(path, index.as_bytes())?;
            Ok(index)
        }
        Ok(_) => cached.ok_or_else(|| boxed_error("AMLL-DB 索引内容无效或不完整")),
        Err(error) => {
            if let Some(index) = cached {
                eprintln!("[Lyrics] AMLL-DB 索引刷新失败，使用旧缓存: {error}");
                Ok(index)
            } else {
                Err(error)
            }
        }
    }
}

async fn download_utf8(
    url: &str,
    resource: &str,
    timeout: Duration,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let response = client()
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| {
            boxed_error(format!(
                "AMLL-DB {resource}请求失败: {}",
                error_chain(&error)
            ))
        })?
        .error_for_status()
        .map_err(|error| {
            boxed_error(format!(
                "AMLL-DB {resource}返回错误状态: {}",
                error_chain(&error)
            ))
        })?;
    let bytes = response.bytes().await.map_err(|error| {
        boxed_error(format!(
            "AMLL-DB {resource}响应体下载失败: {}",
            error_chain(&error)
        ))
    })?;
    String::from_utf8(bytes.to_vec())
        .map_err(|error| boxed_error(format!("AMLL-DB {resource}不是有效 UTF-8: {error}")))
}

fn cache_is_fresh(path: &Path, max_age: Duration) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age <= max_age)
}

fn index_is_valid(index: &str) -> bool {
    index
        .lines()
        .filter(|line| {
            serde_json::from_str::<Value>(line)
                .ok()
                .is_some_and(|value| value.get("id").is_some() && value.get("metadata").is_some())
        })
        .take(10)
        .count()
        == 10
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<(), Box<dyn Error + Send + Sync>> {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_path = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
    fs::write(&temp_path, content)?;

    if path.exists() {
        fs::remove_file(path)?;
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

fn boxed_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::other(message.into()))
}

fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut messages = vec![error.to_string()];
    let mut source = error.source();
    while let Some(error) = source {
        messages.push(error.to_string());
        source = error.source();
    }
    messages.dedup();
    messages.join(": ")
}

struct Candidate {
    id: String,
    raw_lyric_file: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    score: usize,
}

fn find_candidate(
    index: &str,
    track: &TrackIdentity,
) -> Result<Option<Candidate>, Box<dyn Error + Send + Sync>> {
    let title = normalize(&track.title);
    let artist = normalize(&track.artist);
    let mut best: Option<Candidate> = None;
    for line in index.lines() {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !is_safe_id(id) {
            continue;
        }
        let Some(metadata) = value.get("metadata").and_then(Value::as_array) else {
            continue;
        };
        let mut candidate = Candidate {
            id: id.to_string(),
            raw_lyric_file: value
                .get("rawLyricFile")
                .and_then(Value::as_str)
                .map(str::to_string),
            title: None,
            artist: None,
            album: None,
            score: 0,
        };
        for entry in metadata {
            let Some(pair) = entry.as_array() else {
                continue;
            };
            let Some(key) = pair.first().and_then(Value::as_str) else {
                continue;
            };
            let values = pair
                .get(1)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let first = values.first().and_then(Value::as_str).map(str::to_string);
            match key {
                "musicName" => candidate.title = first,
                "artists" => candidate.artist = first,
                "album" => candidate.album = first,
                "appleMusicId" => {}
                _ => {}
            }
        }
        let Some(candidate_title) = candidate.title.as_deref() else {
            continue;
        };
        let Some(candidate_artist) = candidate.artist.as_deref() else {
            continue;
        };
        if normalize(candidate_title) == title {
            candidate.score += 2;
        }
        if normalize(candidate_artist).contains(&artist)
            || artist.contains(&normalize(candidate_artist))
        {
            candidate.score += 2;
        }
        if candidate.score < 4 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|current| candidate.score > current.score)
        {
            best = Some(candidate);
        }
    }
    Ok(best)
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .replace(['-', '_', '(', ')', '[', ']', ',', '.', '!', '?'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn is_safe_ttml_file(path: &str) -> bool {
    let file = Path::new(path);
    file.components().count() == 1
        && file
            .extension()
            .is_some_and(|extension| extension == "ttml")
        && file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| is_safe_id(name.trim_end_matches(".ttml")))
}

fn cache_dir() -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    crate::app_paths::lyrics_dir().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_matching_index_candidate() {
        let index = r#"{"id":"111","rawLyricFile":"111.ttml","metadata":[["musicName",["Other"]],["artists",["Artist"]]]}
    {"id":"222","rawLyricFile":"222.ttml","metadata":[["musicName",["Test Song"]],["artists",["Test Artist"]],["album",["Test Album"]]]}"#;
        let track = TrackIdentity {
            title: "Test Song".into(),
            artist: "Test Artist".into(),
            album: None,
        };

        let candidate = find_candidate(index, &track).unwrap().unwrap();

        assert_eq!(candidate.id, "222");
        assert_eq!(candidate.album.as_deref(), Some("Test Album"));
    }

    #[test]
    fn rejects_unsafe_candidate_id() {
        let index = r#"{"id":"../unsafe","rawLyricFile":"unsafe.ttml","metadata":[["musicName",["Test Song"]],["artists",["Test Artist"]]]}"#;
        let track = TrackIdentity {
            title: "Test Song".into(),
            artist: "Test Artist".into(),
            album: None,
        };

        assert!(find_candidate(index, &track).unwrap().is_none());
    }

    #[test]
    #[ignore = "requires live AMLL-DB network access"]
    fn downloads_live_index_and_matches_track() {
        let index =
            tauri::async_runtime::block_on(download_utf8(INDEX_URL, "索引", INDEX_REQUEST_TIMEOUT))
                .unwrap();
        let track = TrackIdentity {
            title: "Fortnight".into(),
            artist: "Taylor Swift".into(),
            album: Some("THE TORTURED POETS DEPARTMENT".into()),
        };

        assert!(index_is_valid(&index));
        assert!(find_candidate(&index, &track).unwrap().is_some());
    }
}
