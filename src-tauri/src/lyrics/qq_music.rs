use std::error::Error;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};

use crate::domain::{LyricLine, LyricsDocument, LyricsSource, TrackIdentity};

const API_URL: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const LEGACY_LYRIC_URL: &str = "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg";

pub async fn fetch_lyrics(
    track: &TrackIdentity,
) -> Result<Option<LyricsDocument>, Box<dyn Error + Send + Sync>> {
    println!("[QQ] 开始获取歌词: track={track:?}");
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(30))
        .http1_only()
        .user_agent("AM-Desktop-Lyrics/0.1")
        .build()?;
    println!("[QQ] HTTP 客户端已创建: http1_only=true, connect_timeout=12s, timeout=30s");
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
        return Ok(None);
    };
    println!("[QQ] 搜索返回 {} 个候选", songs.len());
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
    }
    let Some((selected_index, song)) = songs
        .iter()
        .enumerate()
        .find(|(_, song)| candidate_has_identity(song))
    else {
        println!("[QQ] 搜索候选均缺少歌词请求所需的 mid 或 id");
        return Ok(None);
    };
    let Some(song_mid) = song.get("mid").and_then(Value::as_str) else {
        println!("[QQ] 匹配候选缺少 mid");
        return Ok(None);
    };
    let Some(song_id) = song.get("id").and_then(Value::as_u64) else {
        println!("[QQ] 匹配候选缺少 id");
        return Ok(None);
    };
    println!(
        "[QQ] 按搜索结果顺序选中候选 #{}: mid={song_mid:?}, id={song_id}, diagnostic_matched={}",
        selected_index + 1,
        song_matches(song, track)
    );

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
        duration_ms: None,
        lines,
    }))
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
    use super::{fetch_lyrics, parse_lrc, qq_artist_query};
    use crate::domain::{LyricsSource, TrackIdentity};

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
