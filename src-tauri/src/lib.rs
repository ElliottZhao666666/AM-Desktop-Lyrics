macro_rules! println {
    ($($arg:tt)*) => {{
        ::std::println!($($arg)*);
        crate::logger::write_line(::std::format_args!($($arg)*));
    }};
}

macro_rules! eprintln {
    ($($arg:tt)*) => {{
        ::std::eprintln!($($arg)*);
        crate::logger::write_line(::std::format_args!($($arg)*));
    }};
}

pub mod app_paths;
pub mod domain;
pub mod logger;
pub mod lyrics;
pub mod media;

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, Wry};

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static SETTINGS_IO_LOCK: Mutex<()> = Mutex::new(());
static LYRICS_OFFSET_UPDATE_LOCK: Mutex<()> = Mutex::new(());
static LYRICS_TIME_OFFSET_MS: AtomicI64 = AtomicI64::new(0);
static LYRICS_TIME_OFFSET_REVISION: AtomicU64 = AtomicU64::new(0);

struct WindowLockState {
    locked: AtomicBool,
    menu_item: CheckMenuItem<Wry>,
}

const DEFAULT_THEME_NAME: &str = "AMDL 默认";
const DEFAULT_WINDOW_WIDTH: u32 = 800;
const DEFAULT_WINDOW_HEIGHT: u32 = 150;
const DEFAULT_WINDOW_BOTTOM_MARGIN: i32 = 80;
const DEFAULT_THEME_CSS: &str = r#"
.amdl-lyrics { font-family: "Microsoft YaHei UI", "Segoe UI", sans-serif; font-size: 32px; font-weight: 800; font-style: normal; text-align: center; opacity: 1; }
.amdl-lyric-line { color: #d7d7de; }
.amdl-lyric-line.is-active { color: transparent; background: linear-gradient(90deg, #fb7185, #d946ef, #8b5cf6); background-clip: text; -webkit-background-clip: text; -webkit-text-fill-color: transparent; }
.amdl-progress-fill { background: linear-gradient(90deg, #fb7185, #d946ef); }
"#;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLyricsTimeOffset {
    pub offset_ms: i64,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub auto_convert_traditional_to_simplified: bool,
    #[serde(default = "default_cache_limit")]
    pub local_cache_limit: u8,
    #[serde(default = "default_theme_name")]
    pub theme_name: String,
    #[serde(default = "default_theme_css")]
    pub theme_css: String,
    #[serde(default)]
    pub lyrics_time_offset_ms: i64,
    #[serde(default)]
    pub remember_lyrics_window_bounds: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics_window_bounds: Option<LyricsWindowBounds>,
}

fn default_true() -> bool {
    true
}
fn default_cache_limit() -> u8 {
    10
}
fn default_theme_name() -> String {
    DEFAULT_THEME_NAME.to_string()
}
fn default_theme_css() -> String {
    DEFAULT_THEME_CSS.trim().to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_convert_traditional_to_simplified: true,
            local_cache_limit: 10,
            theme_name: default_theme_name(),
            theme_css: default_theme_css(),
            lyrics_time_offset_ms: 0,
            remember_lyrics_window_bounds: false,
            lyrics_window_bounds: None,
        }
    }
}

fn settings_path() -> Result<std::path::PathBuf, String> {
    app_paths::settings_file()
}

fn read_settings_unlocked() -> Result<AppSettings, String> {
    let path = settings_path()?;
    let encoded = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return Ok(AppSettings::default()),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("设置 Base64 解码失败: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("设置解析失败: {error}"))
}

fn normalize_settings(mut settings: AppSettings) -> AppSettings {
    settings.local_cache_limit = settings.local_cache_limit.clamp(1, 50);
    if settings.theme_name.trim().is_empty() {
        settings.theme_name = default_theme_name();
    }
    if settings.theme_css.trim().is_empty() {
        settings.theme_css = default_theme_css();
    }
    settings
}

fn write_settings_unlocked(settings: &AppSettings) -> Result<(), String> {
    let bytes = serde_json::to_vec(settings).map_err(|error| format!("设置序列化失败: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let path = settings_path()?;
    let parent = path.parent().ok_or_else(|| "设置目录无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("创建设置目录失败: {error}"))?;
    std::fs::write(path, encoded).map_err(|error| format!("写入设置失败: {error}"))
}

#[tauri::command]
fn get_settings() -> Result<AppSettings, String> {
    let _guard = SETTINGS_IO_LOCK
        .lock()
        .map_err(|_| "设置文件锁已损坏".to_string())?;
    read_settings_unlocked()
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let runtime_offset_guard = LYRICS_OFFSET_UPDATE_LOCK
        .lock()
        .map_err(|_| "歌词偏移状态锁已损坏".to_string())?;
    let settings = {
        let _guard = SETTINGS_IO_LOCK
            .lock()
            .map_err(|_| "设置文件锁已损坏".to_string())?;
        let latest_bounds = read_settings_unlocked()
            .unwrap_or_default()
            .lyrics_window_bounds;
        let mut settings = normalize_settings(settings);
        settings.lyrics_time_offset_ms = LYRICS_TIME_OFFSET_MS.load(Ordering::SeqCst);
        settings.lyrics_window_bounds = if settings.remember_lyrics_window_bounds {
            app.get_webview_window("main")
                .and_then(|window| current_window_bounds(&window))
                .or(latest_bounds)
        } else {
            latest_bounds
        };
        write_settings_unlocked(&settings)?;
        settings
    };
    drop(runtime_offset_guard);
    app.emit("settings-changed", &settings)
        .map_err(|error| format!("广播设置失败: {error}"))?;
    Ok(())
}

#[tauri::command]
fn get_runtime_lyrics_time_offset() -> RuntimeLyricsTimeOffset {
    let _guard = LYRICS_OFFSET_UPDATE_LOCK.lock().ok();
    RuntimeLyricsTimeOffset {
        offset_ms: LYRICS_TIME_OFFSET_MS.load(Ordering::SeqCst),
        revision: LYRICS_TIME_OFFSET_REVISION.load(Ordering::SeqCst),
    }
}

fn update_runtime_lyrics_time_offset(
    app: &AppHandle,
    offset_ms: i64,
) -> Result<RuntimeLyricsTimeOffset, String> {
    let _guard = LYRICS_OFFSET_UPDATE_LOCK
        .lock()
        .map_err(|_| "歌词偏移状态锁已损坏".to_string())?;
    LYRICS_TIME_OFFSET_MS.store(offset_ms, Ordering::SeqCst);
    let payload = RuntimeLyricsTimeOffset {
        offset_ms,
        revision: LYRICS_TIME_OFFSET_REVISION.fetch_add(1, Ordering::SeqCst) + 1,
    };
    app.emit("lyrics-time-offset-changed", payload)
        .map_err(|error| format!("广播歌词偏移失败: {error}"))?;
    Ok(payload)
}

#[tauri::command]
fn set_runtime_lyrics_time_offset(
    app: AppHandle,
    offset_ms: i64,
) -> Result<RuntimeLyricsTimeOffset, String> {
    update_runtime_lyrics_time_offset(&app, offset_ms)
}

#[tauri::command]
fn adjust_runtime_lyrics_time_offset(
    app: AppHandle,
    delta_ms: i64,
) -> Result<RuntimeLyricsTimeOffset, String> {
    let _guard = LYRICS_OFFSET_UPDATE_LOCK
        .lock()
        .map_err(|_| "歌词偏移状态锁已损坏".to_string())?;
    let offset_ms = LYRICS_TIME_OFFSET_MS
        .load(Ordering::SeqCst)
        .saturating_add(delta_ms);
    LYRICS_TIME_OFFSET_MS.store(offset_ms, Ordering::SeqCst);
    let payload = RuntimeLyricsTimeOffset {
        offset_ms,
        revision: LYRICS_TIME_OFFSET_REVISION.fetch_add(1, Ordering::SeqCst) + 1,
    };
    app.emit("lyrics-time-offset-changed", payload)
        .map_err(|error| format!("广播歌词偏移失败: {error}"))?;
    Ok(payload)
}

#[derive(Debug, Serialize, Deserialize)]
struct ThemeFile {
    name: String,
    css: String,
}

fn validate_theme(theme: ThemeFile) -> Result<ThemeFile, String> {
    if theme.name.trim().is_empty() {
        return Err("主题名称不能为空".to_string());
    }
    if theme.css.trim().is_empty() || theme.css.len() > 128 * 1024 {
        return Err("主题 CSS 为空或超过 128 KiB".to_string());
    }
    Ok(theme)
}

#[tauri::command]
fn import_theme_file(path: String) -> Result<ThemeFile, String> {
    let encoded =
        std::fs::read_to_string(path).map_err(|error| format!("读取主题失败: {error}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("主题 Base64 解码失败: {error}"))?;
    let theme = serde_json::from_slice::<ThemeFile>(&bytes)
        .map_err(|error| format!("主题内容解析失败: {error}"))?;
    validate_theme(theme)
}

#[tauri::command]
fn export_theme_file(path: String, name: String, css: String) -> Result<(), String> {
    let theme = validate_theme(ThemeFile { name, css })?;
    let bytes = serde_json::to_vec(&theme).map_err(|error| format!("主题序列化失败: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    std::fs::write(path, encoded).map_err(|error| format!("写入主题失败: {error}"))
}

fn keep_settings_window_reusable(window: &tauri::WebviewWindow) {
    let reusable_window = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = reusable_window.hide();
        }
    });
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn get_system_fonts() -> Result<Vec<String>, String> {
    use std::collections::BTreeSet;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts")
        .map_err(|error| format!("读取系统字体注册表失败: {error}"))?;
    let mut fonts = BTreeSet::new();
    for item in key.enum_values() {
        let (name, _) = item.map_err(|error| format!("读取系统字体项失败: {error}"))?;
        let cleaned = name
            .trim()
            .trim_end_matches("(TrueType)")
            .trim_end_matches("(OpenType)")
            .trim()
            .to_string();
        if !cleaned.is_empty() {
            fonts.insert(cleaned);
        }
    }
    Ok(fonts.into_iter().collect())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn get_system_fonts() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html".into()))
        .title("AMDL 设置")
        .inner_size(1160.0, 680.0)
        .min_inner_size(1040.0, 580.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(true)
        .closable(true)
        .decorations(true)
        .center()
        .build()
        .map_err(|error| error.to_string())?;
    keep_settings_window_reusable(&window);
    window.set_focus().map_err(|error| error.to_string())
}

fn position_unlock_window(app: &AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到歌词主窗口".to_string())?;
    let unlock = app
        .get_webview_window("unlock")
        .ok_or_else(|| "找不到解锁窗口".to_string())?;
    let main_position = main.outer_position().map_err(|error| error.to_string())?;
    let main_size = main.outer_size().map_err(|error| error.to_string())?;
    let unlock_size = unlock.outer_size().map_err(|error| error.to_string())?;
    let x = main_position.x + (main_size.width as i32 - unlock_size.width as i32) / 2;
    unlock
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x,
            main_position.y,
        )))
        .map_err(|error| format!("定位解锁窗口失败: {error}"))
}

fn apply_window_lock(
    app: &AppHandle,
    state: &WindowLockState,
    locked: bool,
) -> Result<bool, String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到歌词主窗口".to_string())?;
    let unlock = app
        .get_webview_window("unlock")
        .ok_or_else(|| "找不到解锁窗口".to_string())?;

    if locked {
        position_unlock_window(app)?;
        main.set_ignore_cursor_events(true)
            .map_err(|error| format!("启用鼠标穿透失败: {error}"))?;
        if let Err(error) = unlock.show() {
            let _ = main.set_ignore_cursor_events(false);
            return Err(format!("显示解锁窗口失败: {error}"));
        }
    } else {
        unlock
            .hide()
            .map_err(|error| format!("隐藏解锁窗口失败: {error}"))?;
        main.set_ignore_cursor_events(false)
            .map_err(|error| format!("关闭鼠标穿透失败: {error}"))?;
    }

    state.locked.store(locked, Ordering::SeqCst);
    state
        .menu_item
        .set_checked(locked)
        .map_err(|error| format!("同步托盘锁定状态失败: {error}"))?;
    app.emit("lock-state-changed", locked)
        .map_err(|error| format!("广播锁定状态失败: {error}"))?;
    println!("[AMDL][窗口] 锁定状态已更新: locked={locked}");
    Ok(locked)
}

#[tauri::command]
fn get_lock_state(state: State<'_, WindowLockState>) -> bool {
    state.locked.load(Ordering::SeqCst)
}

#[tauri::command]
fn set_window_locked(
    app: AppHandle,
    state: State<'_, WindowLockState>,
    locked: bool,
) -> Result<bool, String> {
    apply_window_lock(&app, &state, locked)
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    if app
        .try_state::<WindowLockState>()
        .is_some_and(|state| state.locked.load(Ordering::SeqCst))
    {
        let _ = position_unlock_window(app);
        if let Some(window) = app.get_webview_window("unlock") {
            let _ = window.show();
        }
    }
}

fn default_window_bounds(window: &tauri::WebviewWindow) -> LyricsWindowBounds {
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.available_monitors().ok()?.into_iter().next());
    let Some(monitor) = monitor else {
        return LyricsWindowBounds {
            x: 0,
            y: 0,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
        };
    };
    let work_area = monitor.work_area();
    let x = work_area.position.x + (work_area.size.width as i32 - DEFAULT_WINDOW_WIDTH as i32) / 2;
    let y = (work_area.position.y + work_area.size.height as i32
        - DEFAULT_WINDOW_HEIGHT as i32
        - DEFAULT_WINDOW_BOTTOM_MARGIN)
        .max(work_area.position.y);
    LyricsWindowBounds {
        x,
        y,
        width: DEFAULT_WINDOW_WIDTH,
        height: DEFAULT_WINDOW_HEIGHT,
    }
}

fn normalize_window_bounds(
    window: &tauri::WebviewWindow,
    bounds: LyricsWindowBounds,
) -> LyricsWindowBounds {
    let Ok(monitors) = window.available_monitors() else {
        return default_window_bounds(window);
    };
    if monitors.is_empty() {
        return default_window_bounds(window);
    }
    let left = monitors
        .iter()
        .map(|monitor| monitor.position().x)
        .min()
        .unwrap();
    let top = monitors
        .iter()
        .map(|monitor| monitor.position().y)
        .min()
        .unwrap();
    let right = monitors
        .iter()
        .map(|monitor| monitor.position().x + monitor.size().width as i32)
        .max()
        .unwrap();
    let bottom = monitors
        .iter()
        .map(|monitor| monitor.position().y + monitor.size().height as i32)
        .max()
        .unwrap();
    let width = bounds.width.clamp(420, (right - left).max(420) as u32);
    let height = bounds.height.clamp(100, (bottom - top).max(100) as u32);
    LyricsWindowBounds {
        x: bounds.x.clamp(left, (right - width as i32).max(left)),
        y: bounds.y.clamp(top, (bottom - height as i32).max(top)),
        width,
        height,
    }
}

fn apply_window_bounds(window: &tauri::WebviewWindow, bounds: LyricsWindowBounds) {
    let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
        bounds.width,
        bounds.height,
    )));
    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        bounds.x, bounds.y,
    )));
}

fn current_window_bounds(window: &tauri::WebviewWindow) -> Option<LyricsWindowBounds> {
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some(LyricsWindowBounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn schedule_window_bounds_save(
    bounds: LyricsWindowBounds,
    generation: Arc<AtomicU64>,
    suspended: Arc<AtomicBool>,
) {
    let current_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        if suspended.load(Ordering::SeqCst)
            || generation.load(Ordering::SeqCst) != current_generation
        {
            return;
        }
        let Ok(_guard) = SETTINGS_IO_LOCK.lock() else {
            return;
        };
        if suspended.load(Ordering::SeqCst)
            || generation.load(Ordering::SeqCst) != current_generation
        {
            return;
        }
        let Ok(mut settings) = read_settings_unlocked() else {
            return;
        };
        if !settings.remember_lyrics_window_bounds {
            return;
        }
        settings.lyrics_window_bounds = Some(bounds);
        let _ = write_settings_unlocked(&settings);
    });
}

fn resume_window_bounds_saving(suspended: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        suspended.store(false, Ordering::SeqCst);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            logger::initialize(app.handle())?;
            println!("[AMDL][启动] 日志系统已初始化");
            // 在应用启动时创建并启动 SMTC 管理器，
            // 它会在独立线程中监听系统媒体控制事件
            let handle = app.handle().clone();
            media::smtc::SmtcManager::new(handle).start();
            let initial_settings = get_settings().unwrap_or_default();
            LYRICS_TIME_OFFSET_MS.store(initial_settings.lyrics_time_offset_ms, Ordering::SeqCst);
            let bounds_save_suspended = Arc::new(AtomicBool::new(true));
            let bounds_save_generation = Arc::new(AtomicU64::new(0));

            let lock_item =
                CheckMenuItem::with_id(app, "lock", "锁定窗口", true, false, None::<&str>)?;
            app.manage(WindowLockState {
                locked: AtomicBool::new(false),
                menu_item: lock_item.clone(),
            });
            let show_main_interface_item =
                MenuItem::with_id(app, "show-main-interface", "显示主界面", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 AMDL", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &lock_item,
                    &show_main_interface_item,
                    &settings_item,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_item,
                ],
            )?;
            let tray_app = app.handle().clone();
            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&menu)
                .tooltip("AMDL");
            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }
            tray_builder
                .on_tray_icon_event(move |_tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(&tray_app);
                    }
                })
                .build(app)?;

            let menu_app = app.handle().clone();
            let menu_lock_item = lock_item.clone();
            app.on_menu_event(move |_app, event| match event.id().as_ref() {
                "lock" => {
                    let checked = menu_lock_item.is_checked().unwrap_or(false);
                    let state = menu_app.state::<WindowLockState>();
                    if let Err(error) = apply_window_lock(&menu_app, &state, checked) {
                        let previous = state.locked.load(Ordering::SeqCst);
                        let _ = menu_lock_item.set_checked(previous);
                        eprintln!("[AMDL][托盘] 切换窗口锁定失败: {error}");
                    }
                }
                "show-main-interface" => {
                    println!("[AMDL][托盘] 显示主界面");
                    show_main_window(&menu_app);
                }
                "settings" => {
                    println!("[AMDL][托盘] 打开设置");
                    if let Err(error) = open_settings(menu_app.clone()) {
                        eprintln!("[AMDL][托盘] 打开设置失败: {error}");
                    }
                }
                "quit" => {
                    println!("[AMDL][托盘] 退出 AMDL");
                    EXIT_REQUESTED.store(true, Ordering::SeqCst);
                    menu_app.exit(0);
                }
                _ => {}
            });

            if let Some(window) = app.get_webview_window("main") {
                let initial_bounds = if initial_settings.remember_lyrics_window_bounds {
                    initial_settings.lyrics_window_bounds
                } else {
                    None
                }
                .unwrap_or_else(|| default_window_bounds(&window));
                apply_window_bounds(&window, normalize_window_bounds(&window, initial_bounds));

                let bounded_window = window.clone();
                let event_save_suspended = bounds_save_suspended.clone();
                let event_save_generation = bounds_save_generation.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if !EXIT_REQUESTED.load(Ordering::SeqCst) {
                            api.prevent_close();
                            let _ = bounded_window.hide();
                            if let Some(unlock) =
                                bounded_window.app_handle().get_webview_window("unlock")
                            {
                                let _ = unlock.hide();
                            }
                        }
                        return;
                    }
                    if !matches!(
                        event,
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
                    ) {
                        return;
                    }
                    let Ok(monitors) = bounded_window.available_monitors() else {
                        return;
                    };
                    if monitors.is_empty() {
                        return;
                    }
                    let Ok(position) = bounded_window.outer_position() else {
                        return;
                    };
                    let Ok(size) = bounded_window.outer_size() else {
                        return;
                    };
                    let left = monitors
                        .iter()
                        .map(|monitor| monitor.position().x)
                        .min()
                        .unwrap();
                    let top = monitors
                        .iter()
                        .map(|monitor| monitor.position().y)
                        .min()
                        .unwrap();
                    let right = monitors
                        .iter()
                        .map(|monitor| monitor.position().x + monitor.size().width as i32)
                        .max()
                        .unwrap();
                    let bottom = monitors
                        .iter()
                        .map(|monitor| monitor.position().y + monitor.size().height as i32)
                        .max()
                        .unwrap();
                    let max_x = (right - size.width as i32).max(left);
                    let max_y = (bottom - size.height as i32).max(top);
                    let x = position.x.clamp(left, max_x);
                    let y = position.y.clamp(top, max_y);
                    if x != position.x || y != position.y {
                        let _ = bounded_window.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(x, y),
                        ));
                        return;
                    }
                    let _ = position_unlock_window(bounded_window.app_handle());
                    if !event_save_suspended.load(Ordering::SeqCst) {
                        schedule_window_bounds_save(
                            LyricsWindowBounds {
                                x,
                                y,
                                width: size.width,
                                height: size.height,
                            },
                            event_save_generation.clone(),
                            event_save_suspended.clone(),
                        );
                    }
                });
                resume_window_bounds_saving(bounds_save_suspended.clone());
            }
            if let Some(window) = app.get_webview_window("settings") {
                keep_settings_window_reusable(&window);
            }
            if let Some(window) = app.get_webview_window("unlock") {
                let _ = window.hide();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            media::smtc::get_media_snapshot,
            media::smtc::play_pause,
            media::smtc::next_track,
            media::smtc::prev_track,
            get_settings,
            save_settings,
            get_runtime_lyrics_time_offset,
            set_runtime_lyrics_time_offset,
            adjust_runtime_lyrics_time_offset,
            import_theme_file,
            export_theme_file,
            open_settings,
            get_system_fonts,
            get_lock_state,
            set_window_locked,
            logger::read_current_log,
            logger::open_log_dir,
            lyrics::provider::search_qq_lyrics,
            lyrics::provider::preview_qq_lyrics,
            lyrics::provider::rescan_local_lyrics,
            lyrics::provider::refetch_qq_lyrics,
            lyrics::provider::apply_manual_lyrics,
            lyrics::provider::apply_custom_lyrics,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
