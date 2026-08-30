pub mod domain;
pub mod lyrics;
pub mod media;

use crate::domain::MediaStatePayload;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

const DEFAULT_THEME_NAME: &str = "AMDL 默认";
const DEFAULT_THEME_CSS: &str = r#"
.amdl-lyrics { font-family: "Microsoft YaHei UI", "Segoe UI", sans-serif; font-size: 32px; font-weight: 800; font-style: normal; text-align: center; opacity: 1; }
.amdl-lyric-line { color: #d7d7de; -webkit-text-stroke: 0px transparent; text-shadow: 0 1px 2px #00000099; }
.amdl-lyric-line.is-active { color: transparent; background: linear-gradient(90deg, #fb7185, #d946ef, #8b5cf6); background-clip: text; -webkit-background-clip: text; -webkit-text-fill-color: transparent; -webkit-text-stroke: 0px transparent; text-shadow: 0 1px 2px #000000aa, 0 0 6px #d946ef66; }
.amdl-progress-fill { background: linear-gradient(90deg, #fb7185, #d946ef); }
"#;

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
        }
    }
}

fn settings_path() -> Result<std::path::PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| {
            std::path::PathBuf::from(path)
                .join("AM-Desktop-Lyrics")
                .join("settings.json")
        })
        .ok_or_else(|| "LOCALAPPDATA 不可用".to_string())
}

#[tauri::command]
fn get_settings() -> Result<AppSettings, String> {
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

#[tauri::command]
fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let mut settings = settings;
    settings.local_cache_limit = settings.local_cache_limit.clamp(1, 50);
    if settings.theme_name.trim().is_empty() {
        settings.theme_name = default_theme_name();
    }
    if settings.theme_css.trim().is_empty() {
        settings.theme_css = default_theme_css();
    }
    let bytes =
        serde_json::to_vec(&settings).map_err(|error| format!("设置序列化失败: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let path = settings_path()?;
    let parent = path.parent().ok_or_else(|| "设置目录无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("创建设置目录失败: {error}"))?;
    std::fs::write(path, encoded).map_err(|error| format!("写入设置失败: {error}"))?;
    app.emit("settings-changed", &settings)
        .map_err(|error| format!("广播设置失败: {error}"))
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

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html".into()))
        .title("AMDL 设置")
        .inner_size(920.0, 700.0)
        .min_inner_size(820.0, 620.0)
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

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 在应用启动时创建并启动 SMTC 管理器，
            // 它会在独立线程中监听系统媒体控制事件
            let handle = app.handle().clone();
            media::smtc::SmtcManager::new(handle).start();

            let title_item =
                MenuItem::with_id(app, "current-title", "未在播放", false, None::<&str>)?;
            let artist_item = MenuItem::with_id(app, "current-artist", "", false, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &title_item,
                    &artist_item,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "prev", "上一曲", true, None::<&str>)?,
                    &MenuItem::with_id(app, "play-pause", "播放/暂停", true, None::<&str>)?,
                    &MenuItem::with_id(app, "next", "下一曲", true, None::<&str>)?,
                    &MenuItem::with_id(app, "lock", "锁定窗口", true, None::<&str>)?,
                    &MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "quit", "退出软件", true, None::<&str>)?,
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

            let title_item_for_media = title_item.clone();
            let artist_item_for_media = artist_item.clone();
            app.listen("media-state-changed", move |event| {
                if let Ok(payload) = serde_json::from_str::<MediaStatePayload>(event.payload()) {
                    let _ = title_item_for_media.set_text(payload.track.title);
                    let _ = artist_item_for_media.set_text(payload.track.artist);
                }
            });

            let menu_app = app.handle().clone();
            app.on_menu_event(move |_app, event| match event.id().as_ref() {
                "prev" => {
                    let _ = media::smtc::prev_track();
                }
                "play-pause" => {
                    let _ = media::smtc::play_pause();
                }
                "next" => {
                    let _ = media::smtc::next_track();
                }
                "settings" => {
                    let _ = open_settings(menu_app.clone());
                }
                "lock" => {
                    let _ = menu_app.emit("tray-toggle-lock", ());
                }
                "quit" => {
                    EXIT_REQUESTED.store(true, Ordering::SeqCst);
                    menu_app.exit(0);
                }
                _ => {}
            });

            // 窗口定位逻辑：将窗口附着在主显示器工作区底部居中
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let work_area = monitor.work_area();
                    let window_size = window
                        .outer_size()
                        .unwrap_or_else(|_| tauri::PhysicalSize::new(800, 150));

                    let x = work_area.position.x
                        + (work_area.size.width as i32 - window_size.width as i32) / 2;
                    let y = work_area.position.y + work_area.size.height as i32
                        - window_size.height as i32;

                    let _ = window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                }

                let bounded_window = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if !EXIT_REQUESTED.load(Ordering::SeqCst) {
                            api.prevent_close();
                            let _ = bounded_window.hide();
                        }
                        return;
                    }
                    if !matches!(event, tauri::WindowEvent::Moved(_)) {
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
                    }
                });
            }
            if let Some(window) = app.get_webview_window("settings") {
                keep_settings_window_reusable(&window);
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
            import_theme_file,
            export_theme_file,
            open_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
