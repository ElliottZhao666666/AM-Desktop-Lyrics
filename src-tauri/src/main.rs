// Windows 的 Debug 和 Release 均作为桌面程序启动；诊断信息仍写入日志文件。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    tauri_app_lib::run()
}
