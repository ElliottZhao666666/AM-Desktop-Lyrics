use base64::Engine;
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

static IO_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn read() -> Result<Value, String> {
    let _guard = IO_LOCK.lock().map_err(|_| "配置文件锁已损坏".to_string())?;
    read_at(&crate::app_paths::settings_file()?)
}

pub fn update(update: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let _guard = IO_LOCK.lock().map_err(|_| "配置文件锁已损坏".to_string())?;
    update_at(&crate::app_paths::settings_file()?, update)
}

fn read_at(path: &Path) -> Result<Value, String> {
    let encoded = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Map::new()))
        }
        Err(error) => return Err(format!("读取配置失败: {error}")),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("配置 Base64 解码失败: {error}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("配置解析失败: {error}"))?;
    if !value.is_object() {
        return Err("配置根节点必须是对象".into());
    }
    Ok(value)
}

fn update_at(path: &Path, update: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let mut value = read_at(path)?;
    update(value.as_object_mut().unwrap());
    let bytes = serde_json::to_vec(&value).map_err(|error| format!("配置序列化失败: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    atomic_write(path, encoded.as_bytes())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "数据目录无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("创建数据目录失败: {error}"))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // 同目录 rename 在 Windows 也会替换目标文件，不先删除原文件。
        std::fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("保存数据失败: {error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_updates_preserve_config_offset_bounds_and_unknown_fields() {
        let path = std::env::temp_dir().join(format!(
            "amdl-settings-{}-{}.json",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        update_at(&path, |root| { root.insert("config".into(), serde_json::json!({"amApiTokens":{"authToken":"synthetic"},"lyricsSources":[{"id":"qqMusic","enabled":false}]})); root.insert("futureField".into(),serde_json::json!(42)); }).unwrap();
        update_at(&path, |root| {
            root.insert("lyricsTimeOffsetMs".into(), serde_json::json!(-1500));
            root.insert("themeName".into(), serde_json::json!("主题"));
        })
        .unwrap();
        update_at(&path, |root| {
            root.insert(
                "lyricsWindowBounds".into(),
                serde_json::json!({"x":100,"y":200}),
            );
        })
        .unwrap();
        let result = read_at(&path).unwrap();
        assert_eq!(result["config"]["amApiTokens"]["authToken"], "synthetic");
        assert_eq!(result["config"]["lyricsSources"][0]["enabled"], false);
        assert_eq!(result["lyricsTimeOffsetMs"], -1500);
        assert_eq!(result["futureField"], 42);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_configuration_is_preserved_instead_of_overwritten() {
        let path = std::env::temp_dir().join(format!("amdl-corrupt-{}.json", std::process::id()));
        std::fs::write(&path, "not base64!").unwrap();
        assert!(update_at(&path, |_| {}).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not base64!");
        std::fs::remove_file(path).unwrap();
    }
}
