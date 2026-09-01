<div align="center">

<img src="src-tauri/icons/icon.png" width="100" alt="AMDL Logo" />

# Apple Music Desktop Lyrics (AMDL)

**专为 Windows 版 Apple Music 打造的现代化桌面歌词工具**

[![Tauri](https://img.shields.io/badge/Tauri-2.0-24C8D8?logo=tauri&logoColor=white)](#)
[![Vue](https://img.shields.io/badge/Vue-3.x-4FC08D?logo=vuedotjs&logoColor=white)](#)
[![Rust](https://img.shields.io/badge/Rust-1.70+-000000?logo=rust&logoColor=white)](#)
[![Platform](https://img.shields.io/badge/Platform-Windows_10%20%7C%2011-0078D6?logo=windows&logoColor=white)](#)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](#)

[官网 (amdl.ybjun.com)](https://amdl.ybjun.com) • [反馈 Bug](mailto://main@ybjun.com) • [提交特性请求](#)

</div>

---

## ✨ 简介

Windows 原生 Apple Music 客户端虽然体验优秀，但一直缺失国人习惯的“桌面悬浮歌词”功能。**AMDL (Apple Music Desktop Lyrics)** 旨在填补这一空白。

借助系统级 SMTC 媒体监听，结合高性能的 Rust + Tauri 架构，AMDL 能够在极低资源占用的前提下，为你提供丝滑、通透、极具张力的动态桌面歌词体验。

## 🚀 核心特性

- **原生级无感运行**：不入侵客户端，纯读缓存和 SMTC 状态，绝对安全稳定。
- **高度视觉自定义**：
  - 🎨 彻底解耦的外观配置，支持多种效果。
  - 📂 支持自定义/系统字体加载，支持导出导入 `.amdlcss` 主题文件。
  - 🎁 内置多款预设主题（暖橙日落、青绿森林、海蓝霓虹、纯白极简等）。
- **精准歌词解析**：
  - 🎤 支持 TTML/LRC 逐词高亮解析。
  - 🔍 内置 QQ 音乐搜刮兜底机制，AM 歌词获取不到时也不用担心。
  - 🛠️ 强大的**手动匹配中心**与**永久自定义歌词池**，一次纠错，终身受用。
  - 🌏 支持 AM 默认获取的繁体歌词自动转简体。
- **沉浸式交互**：
  - 🖱️ 支持彻底的**鼠标穿透模式**（系统托盘一键解锁），绝不干扰日常办公。
  - 🎛️ 悬浮控制条无缝集成切歌、暂停等媒体控制。
- **轻量健壮**：
  - 🪶 极低的内存与 CPU 占用。
  - 📝 内置轻量级双文件轮转日志系统，方便排错与追溯。

## 📸 界面预览


| 桌面歌词效果 | 强大的主题编辑器 | 手动匹配 |
| :---: | :---: | :---: |
| ![lyrics-preview](docs/assets/lyrics-preview.png) | ![settings-preview](docs/assets/settings-preview.png) | ![match-preview](docs/assets/match-preview.png) |

## 📥 安装与使用 (Installation & Usage)

### 下载
前往 [Releases](#) 页面下载最新版本的安装包：
- 提供标准的 `.exe` 安装程序。
- 也提供绿色版免安装可执行文件。

### 快速开始
1. 确保你的电脑已开启并正在运行 Windows 版 **Apple Music**。
2. 启动 **AMDL**。
3. 播放任意歌曲，歌词窗口将自动吸附在主显示器底部正中。
4. **控制与设置**：将鼠标悬停在歌词区域上方，唤出控制条点击 ⚙️ 进入设置；或者在系统右下角托盘图标处右键进入。
5. **锁定窗口**：在系统托盘右键菜单中勾选“锁定窗口”，即可实现鼠标完全穿透。

## 🛠️ 本地开发 (Development)

本项目基于 Tauri v2 架构开发，前后端分离。

**环境要求：**
- Node.js (推荐 v18+) & pnpm
- Rust 工具链 (cargo)
- Windows SDK 构建工具

**构建步骤：**
```bash
# 1. 克隆代码仓库
git clone [https://github.com/ElliottZhao666666/AM-Desktop-Lyrics.git](https://github.com/ElliottZhao666666/AM-Desktop-Lyrics.git)
cd AM-Desktop-Lyrics

# 2. 安装前端依赖
pnpm install

# 3. 本地开发调试
pnpm tauri dev

# 4. 编译 Release 版本
pnpm tauri build
```

## 🗺️ 路线图

[x] 核心 SMTC 状态监听与播放时间同步模型

[x] 动态无边框透明窗口

[x] 自身缓存机制、AM 缓存读取与 QQ 音乐 Providers 歌词获取

[x] 多格式歌词解析与展示

[x] 基于 Tailwind Grid 的现代化主题外观配置系统

[x] 自动繁体转换和偏移调节

[x] 双文件轮转本地日志系统

[ ] 双行歌词支持

[ ] TTML 逐词支持、平滑滚动与算法优化

[ ] 更多自定义主题和歌词渠道支持

[ ] 完善更新服务后端

......

## 🤝 贡献与反馈
欢迎提交 Issue 报告 Bug 或分享你的灵感，也随时欢迎提交 Pull Request 一起完善 AMDL！如果在歌词匹配或主题样式上有好的想法，请不要犹豫。

## 📄 许可
本项目采用 MIT License 协议开源。

## Made with ❤️ by [月半菌](https://www.ybjun.com)