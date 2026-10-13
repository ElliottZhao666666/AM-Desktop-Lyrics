use std::error::Error;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::{LyricLine, LyricsDocument, LyricsSource, TrackIdentity};

const API_URL: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const LEGACY_LYRIC_URL: &str = "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QqSongCandidate {
    pub song_id: u64,
    pub song_mid: String,
    pub track: TrackIdentity,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QqLyricsPreview {
    pub candidate: QqSongCandidate,
    pub lyrics_text: String,
}

pub async fn fetch_lyrics(
    track: &TrackIdentity,
) -> Result<Option<LyricsDocument>, Box<dyn Error + Send + Sync>> {
    fetch_matched_lyrics(track, 0).await
}

pub async fn fetch_matched_lyrics(
    track: &TrackIdentity,
    duration_ms: u64,
) -> Result<Option<LyricsDocument>, Box<dyn Error + Send + Sync>> {
    println!("[QQ] 开始获取歌词: track={track:?}");
    let candidates = search_songs(track).await?;
    let mut ranked = rank_candidates(&candidates, track, duration_ms);
    ranked.truncate(3);
    let mut last_error = None;
    for (score, selected) in ranked {
        println!(
            "[QQ] 尝试可信候选: mid={}, score={score}",
            selected.song_mid
        );
        match fetch_lyrics_by_song(track, selected).await {
            Ok(Some(document)) => return Ok(Some(document)),
            Ok(None) => {}
            Err(error) => last_error = Some(error),
        }
    }
    if let Some(error) = last_error {
        return Err(error);
    }
    Ok(None)
}

fn rank_candidates<'a>(
    candidates: &'a [QqSongCandidate],
    track: &TrackIdentity,
    duration_ms: u64,
) -> Vec<(u16, &'a QqSongCandidate)> {
    let mut ranked = candidates
        .iter()
        .filter_map(|candidate| {
            let score = crate::lyrics::matching::score_candidate(
                track,
                duration_ms,
                &candidate.track,
                candidate.duration_ms,
            );
            score.acceptable.then_some((score.total, candidate))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    ranked
}

pub async fn search_songs(
    track: &TrackIdentity,
) -> Result<Vec<QqSongCandidate>, Box<dyn Error + Send + Sync>> {
    let client = create_client()?;
    let qq_artist = qq_artist_query(&track.artist);
    let search_query = if qq_artist.is_empty() {
        track.title.clone()
    } else {
        format!("{} {}", track.title, qq_artist)
    };
    let search_body = json!({
        "comm": { "ct": "19", "cv": "1859", "uin": "0" },
        "req_0": {
            "module": "music.search.SearchCgiService",
            "method": "DoSearchForQQMusicDesktop",
            "param": {
                "grp": 1,
                "num_per_page": 10,
                "page_num": 1,
                "query": search_query,
                "search_type": 0
            }
        }
    });
    println!(
        "[QQ] 发送搜索请求: endpoint={}, query={:?}, normalized_title={:?}, expected_artist={:?}, normalized_artist={:?}",
        API_URL,
        search_query,
        normalize(&track.title),
        track.artist,
        normalize(&track.artist)
    );
    let value = post_api(&client, &search_body).await?;
    ensure_api_success(&value, "req_0", "搜索")?;
    let songs = value
        .pointer("/req_0/data/body/song/list")
        .and_then(Value::as_array);
    let Some(songs) = songs else {
        println!("[QQ] 搜索响应中没有歌曲列表");
        return Ok(Vec::new());
    };
    println!("[QQ] 搜索返回 {} 个候选", songs.len());
    let mut candidates = Vec::new();
    for (index, song) in songs.iter().enumerate() {
        let title = song.get("name").and_then(Value::as_str).unwrap_or_default();
        let artists = song
            .get("singer")
            .and_then(Value::as_array)
            .map(|singers| {
                singers
                    .iter()
                    .filter_map(|singer| singer.get("name").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        println!(
            "[QQ] 候选 #{}: title={:?}, artists={:?}, mid={:?}, id={:?}, title_normalized={:?}, artist_normalized={:?}, matched={}",
            index + 1,
            title,
            artists,
            song.get("mid").and_then(Value::as_str),
            song.get("id").and_then(Value::as_u64),
            normalize(title),
            normalize(&artists),
            song_matches(song, track)
        );
        if let Some(candidate) = parse_candidate(song) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

pub async fn preview_lyrics(
    candidate: QqSongCandidate,
) -> Result<Option<QqLyricsPreview>, Box<dyn Error + Send + Sync>> {
    let client = create_client()?;
    let lyrics_text = fetch_lyric_text(&client, &candidate).await?;
    if parse_lrc(&lyrics_text).is_empty() {
        return Ok(None);
    }
    Ok(Some(QqLyricsPreview {
        candidate,
        lyrics_text: plain_lyrics(&lyrics_text),
    }))
}

pub async fn fetch_lyrics_by_song(
    track: &TrackIdentity,
    candidate: &QqSongCandidate,
) -> Result<Option<LyricsDocument>, Box<dyn Error + Send + Sync>> {
    let client = create_client()?;
    let lyric = fetch_lyric_text(&client, candidate).await?;
    println!("[QQ] 歌词文本已取得: utf8_length={}", lyric.len());
    let lines = parse_lrc(&lyric);
    if lines.is_empty() {
        println!("[QQ] LRC 解析完成但没有有效时间轴歌词行");
        return Ok(None);
    }
    println!(
        "[QQ] LRC 解析成功: lines={}, first_begin_ms={}, last_begin_ms={}",
        lines.len(),
        lines.first().map(|line| line.begin_ms).unwrap_or_default(),
        lines.last().map(|line| line.begin_ms).unwrap_or_default()
    );
    Ok(Some(LyricsDocument {
        track: track.clone(),
        source: LyricsSource::QqMusic,
        duration_ms: candidate.duration_ms,
        lines,
    }))
}

async fn fetch_lyric_text(
    client: &Client,
    candidate: &QqSongCandidate,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let song_mid = &candidate.song_mid;
    let song_id = candidate.song_id;

    let lyric_body = json!({
        "comm": { "ct": "19", "cv": "1859", "uin": "0" },
        "req_0": {
            "module": "music.musichallSong.PlayLyricInfo",
            "method": "GetPlayLyricInfo",
            "param": {
                "songMID": song_mid,
                "songID": song_id,
                "trans": 0,
                "roma": 0,
                "qrc": 0
            }
        }
    });
    println!("[QQ] 请求统一歌词接口: mid={song_mid:?}, id={song_id}");
    let value = post_api(&client, &lyric_body).await?;
    let lyric = match ensure_api_success(&value, "req_0", "歌词") {
        Ok(()) => match value
            .pointer("/req_0/data/lyric")
            .and_then(Value::as_str)
            .filter(|lyric| !lyric.is_empty())
        {
            Some(encoded) => {
                println!("[QQ] 统一歌词接口命中: base64_length={}", encoded.len());
                String::from_utf8(BASE64.decode(encoded)?)?
            }
            None => {
                println!("[QQ] 统一歌词接口成功但歌词为空，转入兼容接口");
                fetch_legacy_lyric(&client, song_mid).await?
            }
        },
        Err(error) => {
            eprintln!("[Lyrics] {error}，回退 QQ 音乐兼容歌词接口");
            fetch_legacy_lyric(&client, song_mid).await?
        }
    };
    Ok(lyric)
}

fn create_client() -> Result<Client, Box<dyn Error + Send + Sync>> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(30))
        .http1_only()
        .user_agent("AMDL/0.9.0")
        .build()?;
    println!("[QQ] HTTP 客户端已创建: http1_only=true, connect_timeout=12s, timeout=30s");
    Ok(client)
}

fn parse_candidate(song: &Value) -> Option<QqSongCandidate> {
    if !candidate_has_identity(song) {
        return None;
    }
    let artists = song
        .get("singer")
        .and_then(Value::as_array)
        .map(|singers| {
            singers
                .iter()
                .filter_map(|singer| singer.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .unwrap_or_default();
    Some(QqSongCandidate {
        song_id: song.get("id")?.as_u64()?,
        song_mid: song.get("mid")?.as_str()?.to_string(),
        track: TrackIdentity {
            title: song.get("name")?.as_str()?.to_string(),
            artist: artists,
            album: song
                .pointer("/album/name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        },
        duration_ms: song
            .get("interval")
            .and_then(Value::as_u64)
            .map(|seconds| seconds.saturating_mul(1000)),
    })
}

fn plain_lyrics(value: &str) -> String {
    value
        .lines()
        .filter_map(|line| line.rsplit_once(']').map(|(_, text)| text.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ensure_api_success(
    value: &Value,
    request_key: &str,
    stage: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let code = value
        .get(request_key)
        .and_then(|request| request.get("code"))
        .and_then(Value::as_i64)
        .unwrap_or(-1);
    if code == 0 {
        println!("[QQ] {stage} API 状态正常: code=0");
        Ok(())
    } else {
        Err(format!("QQ 音乐{stage} API 返回错误码 {code}").into())
    }
}

async fn fetch_legacy_lyric(
    client: &Client,
    song_mid: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    println!(
        "[QQ] 请求兼容歌词接口: endpoint={}, mid={song_mid:?}",
        LEGACY_LYRIC_URL
    );
    let value: Value = client
        .get(LEGACY_LYRIC_URL)
        .query(&[
            ("songmid", song_mid),
            ("format", "json"),
            ("nobase64", "1"),
            ("g_tk", "5381"),
            ("loginUin", "0"),
            ("hostUin", "0"),
            ("inCharset", "utf8"),
            ("outCharset", "utf-8"),
            ("notice", "0"),
            ("platform", "yqq"),
            ("needNewCode", "0"),
        ])
        .header("Referer", "https://y.qq.com/portal/player.html")
        .header("Origin", "https://y.qq.com")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let code = value.get("retcode").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        return Err(format!("QQ 音乐兼容歌词 API 返回错误码 {code}").into());
    }
    let lyric = value
        .get("lyric")
        .and_then(Value::as_str)
        .filter(|lyric| !lyric.is_empty())
        .map(str::to_string)
        .ok_or_else(|| -> Box<dyn Error + Send + Sync> {
            "QQ 音乐兼容歌词 API 未返回歌词".into()
        })?;
    println!("[QQ] 兼容歌词接口命中: utf8_length={}", lyric.len());
    Ok(lyric)
}

fn song_matches(song: &Value, track: &TrackIdentity) -> bool {
    let title = song.get("name").and_then(Value::as_str).unwrap_or_default();
    let artist = song
        .get("singer")
        .and_then(Value::as_array)
        .map(|singers| {
            singers
                .iter()
                .filter_map(|singer| singer.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    normalize(title) == normalize(&track.title)
        && normalize(&artist).contains(&normalize(&track.artist))
}

fn candidate_has_identity(song: &Value) -> bool {
    song.get("mid").and_then(Value::as_str).is_some()
        && song.get("id").and_then(Value::as_u64).is_some()
}

fn qq_artist_query(artist: &str) -> String {
    artist
        .split(|ch| matches!(ch, ',' | '，' | '&' | '＆' | ';' | '；'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

async fn post_api<T: Serialize + ?Sized>(
    client: &Client,
    body: &T,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let mut last_error = None;
    for attempt in 1..=2 {
        println!("[QQ] POST 请求尝试 {attempt}/2: {API_URL}");
        match client
            .post(API_URL)
            .header("Referer", "https://y.qq.com/")
            .header("Origin", "https://y.qq.com")
            .json(body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                println!("[QQ] POST 响应: status={status}");
                return Ok(response.error_for_status()?.json().await?);
            }
            Err(error) if attempt < 2 && (error.is_connect() || error.is_timeout()) => {
                eprintln!("[Lyrics] QQ 音乐连接失败，正在重试: {error}");
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(last_error
        .expect("QQ Music retry loop must capture an error")
        .into())
}

fn parse_lrc(value: &str) -> Vec<LyricLine> {
    let mut lines = value
        .lines()
        .filter_map(|line| {
            let end = line.find(']')?;
            let timestamp = line.get(1..end)?;
            let mut parts = timestamp.split(':');
            let minutes = parts.next()?.parse::<u64>().ok()?;
            let seconds = parts.next()?.parse::<f64>().ok()?;
            let begin_ms = minutes * 60_000 + (seconds * 1000.0).round() as u64;
            let text = line.get(end + 1..)?.trim().to_string();
            (!text.is_empty()).then_some(LyricLine {
                begin_ms,
                end_ms: None,
                words: Vec::new(),
                text,
            })
        })
        .collect::<Vec<_>>();
    lines.sort_by_key(|line| line.begin_ms);
    for index in 0..lines.len().saturating_sub(1) {
        lines[index].end_ms = Some(lines[index + 1].begin_ms);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{fetch_lyrics, parse_candidate, parse_lrc, plain_lyrics, qq_artist_query};
    use crate::domain::{LyricsSource, TrackIdentity};
    use serde_json::json;

    #[test]
    fn unrelated_first_candidate_is_never_used_as_fallback() {
        let track = TrackIdentity {
            title: "Song".into(),
            artist: "Artist".into(),
            album: None,
        };
        let candidates = vec![
            super::QqSongCandidate {
                song_id: 1,
                song_mid: "wrong".into(),
                track: TrackIdentity {
                    artist: "Other".into(),
                    ..track.clone()
                },
                duration_ms: Some(180000),
            },
            super::QqSongCandidate {
                song_id: 2,
                song_mid: "correct".into(),
                track: track.clone(),
                duration_ms: Some(180000),
            },
        ];
        let ranked = super::rank_candidates(&candidates, &track, 180000);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].1.song_mid, "correct");
        assert!(super::rank_candidates(&candidates[..1], &track, 180000).is_empty());
    }

    #[test]
    fn parses_lrc_timestamps() {
        let lines = parse_lrc("[00:01.50]first\n[00:03.00]second");
        assert_eq!(lines[0].begin_ms, 1500);
        assert_eq!(lines[0].end_ms, Some(3000));
    }

    #[test]
    fn converts_apple_music_artist_separators_for_qq_search() {
        assert_eq!(
            qq_artist_query("歌手甲 , 歌手乙＆歌手丙"),
            "歌手甲;歌手乙;歌手丙"
        );
    }

    #[test]
    fn parses_search_candidate_metadata() {
        let candidate = parse_candidate(&json!({
            "id": 123,
            "mid": "mid123",
            "name": "测试歌曲",
            "singer": [{ "name": "歌手甲" }, { "name": "歌手乙" }],
            "album": { "name": "测试专辑" },
            "interval": 245
        }))
        .expect("parse candidate");

        assert_eq!(candidate.song_id, 123);
        assert_eq!(candidate.song_mid, "mid123");
        assert_eq!(candidate.track.title, "测试歌曲");
        assert_eq!(candidate.track.artist, "歌手甲 / 歌手乙");
        assert_eq!(candidate.track.album.as_deref(), Some("测试专辑"));
        assert_eq!(candidate.duration_ms, Some(245_000));
    }

    #[test]
    fn extracts_plain_lyrics_without_timestamps() {
        assert_eq!(
            plain_lyrics("[00:01.00]第一行\n[00:03.50] 第二行\n[ar:歌手]\n"),
            "第一行\n第二行"
        );
    }

    #[test]
    #[ignore = "requires live QQ Music network access"]
    fn downloads_live_lyrics() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create tokio runtime");
        let document = runtime
            .block_on(fetch_lyrics(&TrackIdentity {
                title: "爱在西元前".to_string(),
                artist: "周杰伦".to_string(),
                album: Some("范特西".to_string()),
            }))
            .expect("request QQ Music lyrics")
            .expect("find matching QQ Music lyrics");

        assert_eq!(document.source, LyricsSource::QqMusic);
        assert!(!document.lines.is_empty());
    }
}
