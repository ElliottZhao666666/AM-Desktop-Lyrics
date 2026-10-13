use crate::domain::TrackIdentity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Mutex;
use std::time::Duration;

use crate::config::AmApiTokens;

const API_ROOT: &str = "https://amp-api.music.apple.com/v1/catalog/cn/songs";
static SONG_METADATA: Mutex<Option<BTreeMap<String, CatalogSong>>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogSong {
    pub id: String,
    pub track: TrackIdentity,
    pub duration_ms: Option<u64>,
}

fn client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent("AM-Desktop-Lyrics")
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(10))
        .build()
}

fn authorize(request: reqwest::RequestBuilder, tokens: &AmApiTokens) -> reqwest::RequestBuilder {
    let mut request = request
        .header("Origin", "https://music.apple.com")
        .header("Referer", "https://music.apple.com/");
    if !tokens.auth_token.trim().is_empty() {
        request = request.header("authorization", &tokens.auth_token);
    }
    if !tokens.media_user_token.trim().is_empty() {
        request = request.header("media-user-token", &tokens.media_user_token);
    }
    request
}

/// lyricsId 不证明属于当前曲目，先查询真实元数据；持久化后也能离线校验。
pub async fn fetch_song_metadata(
    ids: &[String],
    tokens: &AmApiTokens,
) -> Result<BTreeMap<String, CatalogSong>, Box<dyn Error + Send + Sync>> {
    let path = crate::app_paths::data_dir()?.join("apple-song-metadata.json");
    let mut found = BTreeMap::new();
    let mut missing = Vec::new();
    {
        let mut guard = SONG_METADATA.lock().map_err(|_| "歌曲元数据锁已损坏")?;
        let cache = guard.get_or_insert_with(|| {
            std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default()
        });
        for id in ids {
            if let Some(song) = cache.get(id) {
                found.insert(id.clone(), song.clone());
            } else {
                missing.push(id.clone());
            }
        }
    }
    if missing.is_empty() {
        return Ok(found);
    }
    let client = client()?;
    for chunk in missing.chunks(25) {
        let joined = chunk.join(",");
        let request = client
            .get(API_ROOT)
            .query(&[("ids", joined.as_str()), ("l", "zh-Hans-CN")]);
        let catalog = match authorize(request, tokens).send().await {
            Ok(response) if response.status().is_success() => {
                response.json::<serde_json::Value>().await.ok()
            }
            _ => None,
        };
        let mut songs = catalog.as_ref().map(parse_catalog).unwrap_or_default();
        if songs.is_empty() {
            let fallback = client
                .get("https://itunes.apple.com/lookup")
                .query(&[
                    ("id", joined.as_str()),
                    ("entity", "song"),
                    ("country", "cn"),
                ])
                .send()
                .await;
            match fallback {
                Ok(response) if response.status().is_success() => {
                    if let Ok(value) = response.json::<serde_json::Value>().await {
                        songs = parse_itunes(&value);
                    }
                }
                Err(error) if found.is_empty() => return Err(error.into()),
                _ => {}
            }
        }
        songs.retain(|id, _| chunk.contains(id));
        found.extend(songs);
    }
    let mut guard = SONG_METADATA.lock().map_err(|_| "歌曲元数据锁已损坏")?;
    let cache = guard.as_mut().unwrap();
    cache.extend(found.clone());
    if let Ok(bytes) = serde_json::to_vec(cache) {
        if let Err(error) = crate::settings_store::atomic_write(&path, &bytes) {
            eprintln!("[AppleMetadata] 保存元数据失败: {error}");
        }
    }
    Ok(found)
}

fn parse_catalog(value: &serde_json::Value) -> BTreeMap<String, CatalogSong> {
    value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|song| {
            let id = song.get("id")?.as_str()?.to_string();
            let attributes = song.get("attributes")?;
            Some((
                id.clone(),
                CatalogSong {
                    id,
                    track: TrackIdentity {
                        title: attributes.get("name")?.as_str()?.into(),
                        artist: attributes.get("artistName")?.as_str()?.into(),
                        album: attributes
                            .get("albumName")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                    },
                    duration_ms: attributes
                        .get("durationInMillis")
                        .and_then(serde_json::Value::as_u64),
                },
            ))
        })
        .collect()
}

fn parse_itunes(value: &serde_json::Value) -> BTreeMap<String, CatalogSong> {
    value
        .get("results")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|song| {
            let id = song.get("trackId")?.as_u64()?.to_string();
            Some((
                id.clone(),
                CatalogSong {
                    id,
                    track: TrackIdentity {
                        title: song.get("trackName")?.as_str()?.into(),
                        artist: song.get("artistName")?.as_str()?.into(),
                        album: song
                            .get("collectionName")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                    },
                    duration_ms: song
                        .get("trackTimeMillis")
                        .and_then(serde_json::Value::as_u64),
                },
            ))
        })
        .collect()
}

/// 获取 Apple Music 官方 syllable lyrics 原始响应。
/// `tokens.auth_token` 会原样写入 Authorization，不会覆盖用户输入。
pub async fn fetch_syllable_lyrics(
    track_id: &str,
    auto_chs: bool,
    tokens: AmApiTokens,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let track_id = urlencoding::encode(track_id);
    let mut url = format!("{API_ROOT}/{track_id}/syllable-lyrics");
    if auto_chs {
        url.push_str("?l[lyrics]=zh-hans-cn&l[script]=zh-Hans&extend=ttmlLocalizations");
    }

    let response = authorize(client()?.get(url), &tokens).send().await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("Apple Music API 返回 HTTP {status}: {body}").into());
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_metadata_contains_identity_instead_of_only_duration() {
        let value = serde_json::json!({"data":[{"id":"123","attributes":{"name":"玻璃","artistName":"Gareth.T","albumName":"玻璃 demo - Single","durationInMillis":185041}}]});
        let result = parse_catalog(&value);
        assert_eq!(result["123"].track.title, "玻璃");
        assert_eq!(result["123"].duration_ms, Some(185041));
        assert!(parse_catalog(
            &serde_json::json!({"data":[{"id":"123","attributes":{"durationInMillis":185041}}]})
        )
        .is_empty());
    }
}
