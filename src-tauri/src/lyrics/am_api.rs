use std::error::Error;

use crate::config::AmApiTokens;

const API_ROOT: &str = "https://amp-api.music.apple.com/v1/catalog/cn/songs";

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

    let client = reqwest::Client::builder()
        .user_agent("AM-Desktop-Lyrics")
        .build()?;
    let mut request = client.get(url);
    if !tokens.auth_token.trim().is_empty() {
        request = request.header("authorization", tokens.auth_token);
    }
    if !tokens.media_user_token.trim().is_empty() {
        request = request.header("media-user-token", tokens.media_user_token);
    }
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("Apple Music API 返回 HTTP {status}: {body}").into());
    }
    Ok(body)
}