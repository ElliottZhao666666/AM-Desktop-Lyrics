use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

const CURRENT_LOG_NAME: &str = "amdl_current.log";
const PREVIOUS_LOG_NAME: &str = "amdl_previous.log";
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_VIEW_BYTES: u64 = 512 * 1024;

struct LoggerState {
    directory: PathBuf,
    current_path: PathBuf,
    file: Option<File>,
    bytes: u64,
}

static LOGGER: OnceLock<Mutex<LoggerState>> = OnceLock::new();

pub fn initialize(_app: &AppHandle) -> Result<(), String> {
    let directory = crate::app_paths::logs_dir()?;
    fs::create_dir_all(&directory).map_err(|error| format!("创建日志目录失败: {error}"))?;
    migrate_legacy_logs(&directory);

    let current_path = directory.join(CURRENT_LOG_NAME);
    let previous_path = directory.join(PREVIOUS_LOG_NAME);
    if previous_path.exists() {
        fs::remove_file(&previous_path).map_err(|error| format!("删除上一份日志失败: {error}"))?;
    }
    if current_path.exists() {
        fs::rename(&current_path, &previous_path)
            .map_err(|error| format!("轮转当前日志失败: {error}"))?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&current_path)
        .map_err(|error| format!("创建当前日志失败: {error}"))?;
    LOGGER
        .set(Mutex::new(LoggerState {
            directory,
            current_path,
            file: Some(file),
            bytes: 0,
        }))
        .map_err(|_| "日志系统已初始化".to_string())?;
    Ok(())
}

fn migrate_legacy_logs(directory: &Path) {
    let Some(roaming) = std::env::var_os("APPDATA") else {
        return;
    };
    let legacy = PathBuf::from(roaming)
        .join("com.crh38.tauri-app")
        .join("logs");
    if legacy == directory || !legacy.is_dir() {
        return;
    }
    for name in [CURRENT_LOG_NAME, PREVIOUS_LOG_NAME] {
        let source = legacy.join(name);
        let destination = directory.join(name);
        if source.is_file() && !destination.exists() {
            let _ = fs::rename(&source, &destination).or_else(|_| {
                fs::copy(&source, &destination)?;
                fs::remove_file(&source)
            });
        }
    }
    let _ = fs::remove_dir(&legacy);
    if let Some(parent) = legacy.parent() {
        let _ = fs::remove_dir(parent);
    }
}

pub fn write_line(arguments: std::fmt::Arguments<'_>) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let Ok(mut state) = logger.lock() else {
        ::std::eprintln!("[AMDL][日志] 日志文件锁已损坏");
        return;
    };
    let record = format!(
        "[{}] {arguments}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f %:z")
    );
    if let Err(error) = state.append(&record) {
        ::std::eprintln!("[AMDL][日志] 写入日志失败: {error}");
    }
}

impl LoggerState {
    fn append(&mut self, record: &str) -> io::Result<()> {
        let end = record
            .char_indices()
            .map(|(index, _)| index)
            .find(|index| *index >= 64 * 1024)
            .unwrap_or(record.len());
        let record = &record[..end];
        if self.bytes + record.len() as u64 > MAX_LOG_BYTES {
            // Windows 下先关闭句柄，再轮转文件。
            self.file.take();
            let previous = self.directory.join(PREVIOUS_LOG_NAME);
            if previous.exists() {
                fs::remove_file(&previous)?;
            }
            fs::rename(&self.current_path, previous)?;
            self.bytes = 0;
        }
        if self.file.is_none() {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.current_path)?,
            );
        }
        let file = self.file.as_mut().unwrap();
        file.write_all(record.as_bytes())?;
        file.flush()?;
        self.bytes += record.len() as u64;
        Ok(())
    }
}

fn read_tail(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(MAX_VIEW_BYTES)))?;
    let mut bytes = Vec::new();
    file.take(MAX_VIEW_BYTES).read_to_end(&mut bytes)?;
    if length > MAX_VIEW_BYTES {
        let start = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        Ok(format!(
            "（日志过长，显示最近 512 KiB）\n{}",
            String::from_utf8_lossy(&bytes[start..])
        ))
    } else {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn with_state<T>(operation: impl FnOnce(&LoggerState) -> io::Result<T>) -> Result<T, String> {
    let logger = LOGGER
        .get()
        .ok_or_else(|| "日志系统尚未初始化".to_string())?;
    let state = logger.lock().map_err(|_| "日志文件锁已损坏".to_string())?;
    operation(&state).map_err(|error| format!("日志操作失败: {error}"))
}

#[tauri::command]
pub fn read_current_log() -> Result<String, String> {
    with_state(|state| read_tail(&state.current_path))
}

#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    let directory = with_state(|state| Ok(state.directory.clone()))?;
    open_directory(&directory)
}

#[cfg(target_os = "windows")]
fn open_directory(directory: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开日志目录失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_session_rotates_and_view_reads_only_a_bounded_utf8_tail() {
        let directory = std::env::temp_dir().join(format!("amdl-log-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(CURRENT_LOG_NAME);
        let file = File::create(&path).unwrap();
        let mut state = LoggerState {
            directory: directory.clone(),
            current_path: path.clone(),
            file: Some(file),
            bytes: 0,
        };
        for _ in 0..70 {
            state.append(&format!("{}\n", "中".repeat(21000))).unwrap();
        }
        assert!(path.metadata().unwrap().len() <= MAX_LOG_BYTES);
        assert!(directory.join(PREVIOUS_LOG_NAME).metadata().unwrap().len() <= MAX_LOG_BYTES);
        assert!(
            read_tail(&directory.join(PREVIOUS_LOG_NAME)).unwrap().len()
                <= MAX_VIEW_BYTES as usize + 100
        );
        assert!(!read_tail(&directory.join(PREVIOUS_LOG_NAME))
            .unwrap()
            .contains('\u{fffd}'));
        state.file.take();
        fs::remove_file(path).unwrap();
        fs::remove_file(directory.join(PREVIOUS_LOG_NAME)).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}

#[cfg(target_os = "macos")]
fn open_directory(directory: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开日志目录失败: {error}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_directory(directory: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开日志目录失败: {error}"))
}
