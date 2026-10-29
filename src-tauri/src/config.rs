use serde::{Deserialize, Serialize};

const DEFAULT_APPLE_MUSIC_AUTH_TOKEN: &str = "Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6IldlYlBsYXlLaWQifQ.eyJpc3MiOiJBTVBXZWJQbGF5IiwiaWF0IjoxNzkyNzgwNTM0LCJleHAiOjE3OTg4Mjg1MzQsInJvb3RfaHR0cHNfb3JpZ2luIjpbImFwcGxlLmNvbSJdfQ.wcf_7U-zcgUjY2L8d4HWFkdxP1YSJCkkqEpEtVPKpRARrlaag0DHyIpRcoUEWZU8kJlvx5Q7ZNp04OabfuiuAw";

// 仅迁移 1.0.1 随附的已过期公开默认值；用户自行填写的其它凭证保持原值。
const LEGACY_APPLE_MUSIC_AUTH_TOKEN: &str = "Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6IldlYlBsYXlLaWQifQ.eyJpc3MiOiJBTVBXZWJQbGF5IiwiaWF0IjoxNzg2NjMyOTI0LCJleHAiOjE3OTI2ODA5MjQsInJvb3RfaHR0cHNfb3JpZ2luIjpbImFwcGxlLmNvbSJdfQ.hBgj61sZf-y7bmuvT-joXAUAcf7TVJ51732xnH5vFkLHOmsQHxVqGMYUuI4h8c0-RX3fRY3moylhLW8fewFJyw";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSource {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmApiTokens {
    #[serde(default = "default_auth_token")]
    pub auth_token: String,
    #[serde(default)]
    pub media_user_token: String,
}

impl Default for AmApiTokens {
    fn default() -> Self {
        Self {
            auth_token: default_auth_token(),
            media_user_token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default = "default_lyrics_sources")]
    pub lyrics_sources: Vec<LyricsSource>,
    #[serde(default)]
    pub am_api_tokens: AmApiTokens,
    #[serde(default = "default_auto_chs")]
    pub auto_chs: bool,
}

fn default_auto_chs() -> bool {
    true
}

fn default_auth_token() -> String {
    DEFAULT_APPLE_MUSIC_AUTH_TOKEN.to_string()
}

fn default_lyrics_sources() -> Vec<LyricsSource> {
    vec![
        LyricsSource {
            id: "manual".into(),
            name: "手动匹配池".into(),
            enabled: true,
            priority: 1,
        },
        LyricsSource {
            id: "appleMusicApi".into(),
            name: "Apple Music API".into(),
            enabled: true,
            priority: 2,
        },
        LyricsSource {
            id: "appleMusicCache".into(),
            name: "Apple Music 本地缓存".into(),
            enabled: true,
            priority: 3,
        },
        LyricsSource {
            id: "qqMusic".into(),
            name: "QQ 音乐".into(),
            enabled: true,
            priority: 4,
        },
        LyricsSource {
            id: "neteaseMusic".into(),
            name: "网易云音乐".into(),
            enabled: false,
            priority: 5,
        },
    ]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            lyrics_sources: default_lyrics_sources(),
            am_api_tokens: AmApiTokens::default(),
            auto_chs: true,
        }
    }
}

impl AppConfig {
    pub fn normalize(mut self) -> Self {
        if self.am_api_tokens.auth_token.trim() == LEGACY_APPLE_MUSIC_AUTH_TOKEN {
            self.am_api_tokens.auth_token = default_auth_token();
        }
        let defaults = default_lyrics_sources();
        for default_source in defaults {
            if let Some(source) = self
                .lyrics_sources
                .iter_mut()
                .find(|source| source.id == default_source.id)
            {
                source.name = default_source.name;
            } else {
                self.lyrics_sources.push(default_source);
            }
        }
        self.lyrics_sources.sort_by_key(|source| source.priority);
        self
    }

    pub fn source_enabled(&self, id: &str) -> bool {
        self.lyrics_sources
            .iter()
            .find(|source| source.id == id)
            .map(|source| source.enabled)
            .unwrap_or(false)
    }
}

pub fn read_config() -> Result<AppConfig, String> {
    let value = crate::settings_store::read()?;
    let mut config = match value.get("config").cloned() {
        Some(value) => serde_json::from_value(value)
            .map(|config: AppConfig| config.normalize())
            .map_err(|error| format!("配置结构解析失败: {error}")),
        None => Ok(AppConfig::default()),
    }?;
    if let Some(enabled) = value
        .get("autoConvertTraditionalToSimplified")
        .and_then(serde_json::Value::as_bool)
    {
        config.auto_chs = enabled;
    }
    Ok(config)
}

pub fn write_config(config: &AppConfig) -> Result<(), String> {
    let value = serde_json::to_value(config).map_err(|error| format!("配置序列化失败: {error}"))?;
    crate::settings_store::update(|root| {
        root.insert("config".into(), value);
        root.insert(
            "autoConvertTraditionalToSimplified".into(),
            config.auto_chs.into(),
        );
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperTokenExpiry {
    pub expires_at: String,
    pub expired: bool,
    pub remaining_seconds: i64,
}

/// 只读取 JWT 声明，不上传凭证，也不把解析成功当作签名或授权有效的证明。
#[tauri::command]
pub fn inspect_apple_developer_token(token: String) -> Result<DeveloperTokenExpiry, String> {
    token_expiry_at(&token, chrono::Utc::now().timestamp())
}

fn token_expiry_at(token: &str, now: i64) -> Result<DeveloperTokenExpiry, String> {
    use base64::Engine;
    let token = token.trim();
    let token = token
        .strip_prefix("Bearer ")
        .or_else(|| token.strip_prefix("bearer "))
        .unwrap_or(token)
        .trim();
    if token.len() > 16 * 1024 {
        return Err("Token 过长，请检查输入是否完整且仅包含开发者 Token".into());
    }
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return Err("无法读取有效期：请输入完整的开发者 JWT Token".into());
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(parts[1]))
        .map_err(|_| "无法解析 Token，请检查输入是否完整".to_string())?;
    let claims: serde_json::Value =
        serde_json::from_slice(&decoded).map_err(|_| "Token 内容格式错误".to_string())?;
    let exp = claims
        .get("exp")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or("Token 中没有有效的到期时间（exp）")?;
    let date = chrono::DateTime::from_timestamp(exp, 0).ok_or("Token 的到期时间超出有效范围")?;
    Ok(DeveloperTokenExpiry {
        expires_at: date.to_rfc3339(),
        expired: exp <= now,
        remaining_seconds: exp.saturating_sub(now).max(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_respects_disabled_sources() {
        let mut config = AppConfig::default();
        for source in &mut config.lyrics_sources {
            source.enabled = false;
        }
        assert!(config
            .normalize()
            .lyrics_sources
            .iter()
            .all(|source| !source.enabled));
    }
    #[test]
    fn normalization_updates_legacy_names_without_changing_source_preferences() {
        let mut config = AppConfig::default();
        config.lyrics_sources[0].name = "手动匹配结果".into();
        config.lyrics_sources[0].enabled = false;
        config.lyrics_sources[0].priority = 7;
        let config = config.normalize();
        let manual = config
            .lyrics_sources
            .iter()
            .find(|source| source.id == "manual")
            .unwrap();
        assert_eq!(manual.name, "手动匹配池");
        assert!(!manual.enabled);
        assert_eq!(manual.priority, 7);
    }
    #[test]
    fn token_defaults_are_identical_for_new_and_legacy_configuration() {
        let deserialized: AmApiTokens = serde_json::from_str("{}").unwrap();
        assert_eq!(AmApiTokens::default().auth_token, deserialized.auth_token);
        assert_eq!(
            AppConfig::default().am_api_tokens.auth_token,
            deserialized.auth_token
        );
    }

    #[test]
    fn expired_bundled_token_is_migrated_without_overwriting_custom_tokens() {
        let mut config = AppConfig::default();
        config.am_api_tokens.auth_token = LEGACY_APPLE_MUSIC_AUTH_TOKEN.into();
        config.am_api_tokens.media_user_token = "user-token".into();
        let migrated = config.normalize();
        assert_eq!(migrated.am_api_tokens.auth_token, default_auth_token());
        assert_eq!(migrated.am_api_tokens.media_user_token, "user-token");
        let mut custom = migrated;
        custom.am_api_tokens.auth_token = "Bearer custom-value".into();
        assert_eq!(
            custom.normalize().am_api_tokens.auth_token,
            "Bearer custom-value"
        );
    }

    #[test]
    fn developer_token_expiry_handles_expired_invalid_and_bearer_values() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let jwt = |claims: serde_json::Value| {
            format!(
                "e30.{}.signature",
                URL_SAFE_NO_PAD.encode(claims.to_string())
            )
        };
        let valid = jwt(serde_json::json!({"exp": 2000}));
        let result = token_expiry_at(&format!("Bearer {valid}"), 1000).unwrap();
        assert!(!result.expired);
        assert_eq!(result.remaining_seconds, 1000);
        assert!(token_expiry_at(&valid, 2000).unwrap().expired);
        assert_eq!(token_expiry_at(&valid, 3000).unwrap().remaining_seconds, 0);
        for value in [
            "".to_string(),
            "Bearer truncated".into(),
            "e30.%%%.signature".into(),
            jwt(serde_json::json!({})),
            jwt(serde_json::json!({"exp":"2000"})),
            jwt(serde_json::json!({"exp":-1})),
            jwt(serde_json::json!({"exp":i64::MAX})),
        ] {
            assert!(token_expiry_at(&value, 1000).is_err());
        }
    }
}
