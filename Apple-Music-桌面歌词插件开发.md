> From: https://gemini.google.com/app/47535b1032413c91

# you asked

message time: 2026-07-09 10:04:24

我现在突然有个想法，我使用 Apple Music听歌（之后简称AM），在Windows上使用的是官方的 AM客户端，但是官方AM客户端没有类似中国音乐平台一样的桌面歌词功能，听歌不能直接在桌面显示歌词就很难受。我现在想通过vibe coding 开发一个桌面歌词插件或软件，当AM播放时可实时显示桌面歌词，只适用于Windows即可，因为AM客户端貌似也只支持Win10/11，所以我们的开发也无需考虑兼容性。
我认为主要分为几部分：歌词获取部分、歌词渲染部分、设置UI部分。现在我们主要看歌词抓取部分。
请你帮我评估一下这个需求，以及具体的技术栈，和技术实现路径。特别是歌词抓取逻辑，可以怎么做。你先基于你的知识库结合网络相关资料分析分析

---

# gemini response

这是一个非常实用且完全可行的项目。Windows 上的 Apple Music 客户端实际上是一个现代的 Windows 应用程序，它原生接入了 Windows 系统的媒体控制接口。这意味着我们**完全不需要**去破解或反编译 AM 客户端，而是可以通过 Windows 官方的系统 API 优雅地获取当前播放状态。

基于 Vibe Coding（与 AI 结对编程）的模式，只要把核心逻辑和技术栈定义清楚，AI 就能极快地生成可用代码。

---

### 技术栈选型建议

针对你的需求以及对工程化代码的要求，这里推荐两套最适合 Vibe Coding 开发该项目的技术栈：

* **方案 A（首选，现代 UI 与性能最佳）：Tauri (Rust) + 前端 (Vue / Svelte)**
    * **优势**：前端 Svelte 或 Vue 极其适合做桌面歌词这种无边框、透明背景、带复杂动画渲染的流体 UI。后端 Rust 通过 `windows` Crate 可以完美、低开销地调用底层 Windows API。打包出来的软件体积小，运行效率极高。
* **方案 B（快速原型，极致敏捷）：Python + PySide6 (或 PyQt6) + winsdk**
    * **优势**：利用 Python 强大的脚本生态，搭配 `winsdk` 库直接抓取 Windows 媒体数据，无论是字符串处理、网络请求还是逻辑编写都极为迅速。PySide6 实现无边框透明窗口的技术也非常成熟。

---

### 核心：歌词抓取与同步逻辑深度解析

开发的核心痛点在于：**如何准确知道 AM 在播什么**，以及**如何精准获取并同步歌词**。整个流程可以拆分为三个独立的工程模块：

#### 1. OS 级数据 Hook：获取当前播放信息 (SMTC API)
Windows 10/11 提供了一个名为 `System Media Transport Controls (SMTC)` 的子系统。当你按下键盘上的音量键时，左上角或正上方弹出的系统媒体悬浮窗，其数据就是来源于此。Apple Music 客户端在播放时会将状态注册到 SMTC。

**原理解析：**
我们需要调用底层的 `Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager` 类。
1.  获取当前活动的媒体会话 (`GetCurrentSession()`)。
2.  通过 `TryGetMediaPropertiesAsync()` 获取 **歌曲名 (Title)** 和 **歌手 (Artist)**。
3.  通过 `GetTimelineProperties()` 获取 **当前播放进度 (Position)** 和状态最后更新的时间戳。

**Python 核心实现示例 (依赖 `winsdk` 库)：**
```python
import asyncio
from winsdk.windows.media.control import GlobalSystemMediaTransportControlsSessionManager

async def get_current_playing_info():
    # 1. 请求 SMTC 管理器实例
    manager = await GlobalSystemMediaTransportControlsSessionManager.request_async()
    
    # 2. 获取当前正在主导系统的媒体会话（即当前播放的 Apple Music）
    session = manager.get_current_session()
    if session is None:
        return None
        
    # 3. 异步获取歌曲元数据
    media_properties = await session.try_get_media_properties_async()
    title = media_properties.title
    artist = media_properties.artist
    
    # 4. 获取时间轴属性（进度）
    timeline = session.get_timeline_properties()
    # 将 Windows 的 TimeSpan 转换为标准的秒数
    position_seconds = timeline.position.total_seconds() 
    
    return {"title": title, "artist": artist, "position": position_seconds}

# 运行: asyncio.run(get_current_playing_info())
```
*(注：如果使用 Tauri，Rust 端的实现逻辑完全对应，仅替换为调用 `windows-rs` 对应的 API 即可。)*

#### 2. 网络级抓取：根据 Metadata 匹配歌词 (.lrc)
由于 Apple Music 本身不开放歌词 API，且自带歌词通常存在加密，业界标准的做法是“曲线救国”：拿到 Title 和 Artist 后，向第三方音乐平台或开源的 LRC 聚合 API 发起检索。

**技术路径：**
1.  **搜索 API 映射**：通过封装好的第三方接口（例如网易云音乐的开源 API 代理，或 LrcApi 等聚合库），使用 URL 编码的 `"{title} {artist}"` 作为关键词搜索。
2.  **精确度控制**：遍历搜索返回的曲目列表。为了避免抓错（比如播的原版，抓到了 Live 版或 Remix 版），需要在代码中引入字符串相似度算法（如 Levenshtein Distance），匹配度最高的才是我们要的 `song_id`。
3.  **获取 LRC 文本**：通过 `song_id` 请求歌词接口，提取出纯文本的 `.lrc` 格式字符串。为了提高体验，可以将下载好的歌词以 SQLite 或本地文件的形式建立缓存，避免切歌时重复请求。

#### 3. 时序解析与精准同步渲染 (LRC Parsing & Sync)
获取到 `.lrc` 字符串后，需要将其解析为程序可读的内存结构，并与 SMTC 获取的进度进行精准对齐。

**LRC 文本解析：**
LRC 标准格式为 `[mm:ss.xx]歌词文本`。
利用正则表达式 `r"\[(\d{2}):(\d{2})\.(\d{2,3})\](.*)"` 可以精准提取。
随后将提取出的时间统一转化为 **毫秒 (ms)** 或 **秒 (s)**，构建出一个类似 `[{ time: 12.05, text: "歌词文本..." }, ...]` 的结构化数组。

**时间平滑插值（渲染核心难点）：**
SMTC 提供的当前时间进度（`position`）**并不是实时高频刷新的**，它通常在暂停、切歌或过一段时间才会更新一次。如果完全依靠轮询 SMTC，歌词滚动会非常卡顿。
* **计算真实渲染进度**：`当前实时播放进度` = `SMTC 提供的 Position` + `(当前系统时钟 - SMTC 状态最后一次更新的系统时钟)`。
* **UI 刷新率**：在前端渲染层（如 Vue/Svelte）利用浏览器的 `requestAnimationFrame`，每帧比对上述计算出的“真实渲染进度”与 LRC 数组中的时间戳。找到对应的行数并触发高亮，配合 CSS 的 `transform: translateY()` 实现顺滑的过渡滚动。

---

如果准备借助 AI 开始写代码，第一步通常是先写出底层的“媒体状态侦听器 (Event Emitter)”。针对这个项目，你更倾向于直接用 Python 快速跑通业务逻辑，还是直接上 Tauri (Rust + 前端) 来追求更好的 UI 渲染上限和独立打包体验？

---

# you asked

message time: 2026-07-09 10:13:12

技术栈选择方案A
关于歌词内容，我记得AM加载歌曲的时候会在本地缓存ttml歌词。如果能获取实时播放状态和本地ttml的话，就能直接渲染了。这方面我找到一个开源项目，代码库我已经附上了，你可以研究一下怎么实现的，好像也用了一些开源库。

---

# gemini response

太棒了，选择 **Tauri (Rust) + Vue/Svelte** 作为技术栈（Plan A）是非常有远见的决定。这套架构不仅能保证极低的系统资源占用，还能在前端实现极具张力的流体动画和动态模糊效果，可以说是复刻 Apple Music 歌词体验的最佳选择。

结合你提供的 `BetterLyrics` 开源项目结构，以及目前开源社区在“Apple Music 桌面歌词”方向的探索（特别是 AMLL - AppleMusic-Like-Lyrics 生态），我们可以对获取 AM 实时播放状态和本地 TTML 歌词的实现路径进行一次非常深度的拆解。

### 核心难点解析：如何拿到并解析 TTML 歌词？

Apple Music 采用的是 **TTML (Timed Text Markup Language)** 的变体格式。与普通的逐行 `.lrc` 不同，AM 的 TTML 带有 `itunes:timing="word"` 属性，它不仅记录了行的 `begin` 和 `end`，还利用 `<span>` 标签记录了每一个音节（Syllable）的时间戳，这是实现“逐字高亮”和“平滑滚动”的灵魂。

在实际工程中，获取这串 TTML 数据有三种主流流派，`BetterLyrics` 的代码库中也印证了这些方案的影子：

#### 1. 内存截取流 (Memory Reading) —— *最硬核、最实时的方案*
在你提供的项目中，有一个非常关键的模块命名为 `UniversalMemoryReaderProvider`。为什么需要读内存？
因为 Windows 版 Apple Music (UWP/WinUI3 架构) 的本地缓存数据存放在类似 `%LocalAppData%\Packages\AppleInc.AppleMusicWin_...\LocalCache` 目录下，但这些缓存文件往往是被加密或动态碎片的。
**实现原理：**
通过 Rust 调用 Windows API (`ReadProcessMemory`)，定位 `AppleMusic.exe` 进程。由于 TTML 在被渲染前必定会以明文（`<?xml version="1.0" encoding="UTF-8"?><tt xmlns="http://www.w3.org/ns/ttml"...`）的形式加载到内存堆中，我们只需通过特征码（Signature Scan）扫描这部分内存，就能直接提取出现正播放歌曲的完整 TTML 文本。这种方式绕过了缓存解密，是目前很多硬核桌面歌词插件的做法。

#### 2. 本地数据库检索流 (AMLL TTML DB)
`BetterLyrics` 代码结构中出现了 `AmllTTmlDB` 的命名。这通常指的是对接了 AMLL 生态。
**实现原理：**
Apple Music Windows 客户端在某些版本中，其 SQLite 数据库或本地存储机制被社区部分破解。但更常见的做法是：利用 SMTC（上一轮提到的系统媒体控制）获取到当前播放的 `Title` 和 `Artist`，然后去本地自己维护的 TTML 数据库库（或者 AMLL 社区开源的 API/本地缓存）中精准匹配。

#### 3. 官方 API 伪装请求流
利用开发者 Token，将获取到的当前播放歌曲信息转换为对 Apple Music Storefront API 的请求，直接拉取官方的 TTML 歌词。虽然需要处理 Token，但这是获取官方最纯净逐字歌词的最稳定途径。

---

### 基于 Tauri + Svelte/Vue 的技术实现全路径

如果你要通过 Vibe Coding 落地这个项目，建议按照以下模块化步骤指挥 AI 编写代码：

#### 第一阶段：后端 (Rust) 的 SMTC Hook 与 内存/API 检索
1. **SMTC 监听器**：
   在 Rust 端引入 `windows` crate，订阅 `GlobalSystemMediaTransportControlsSessionManager` 的 `MediaPropertiesChanged` 和 `PlaybackInfoChanged` 事件。
   *目标*：一旦 AM 换歌或暂停，Rust 立刻感知，并获取到 `Title`, `Artist`, 以及当前的毫秒级 `Position`。
2. **TTML 获取器**：
   你可以先从简单的 API 方案或本地文件加载方案入手，后续再实现复杂的内存读取。
3. **TTML 解析器引擎**：
   强烈建议在 Rust 端使用 `quick-xml` 库，或者直接引入开源社区专为解析 AM 格式打造的 `ttml_processor` crate。
   把复杂的 XML 解析放在性能强悍的 Rust 端，将其拍平转化成如下的 JSON 格式发送给前端：
   ```json
   {
     "lines": [
       {
         "begin": 5000, "end": 10000,
         "syllables": [
           { "text": "Hello ", "begin": 5100, "end": 5500 },
           { "text": "world", "begin": 5600, "end": 6000 }
         ]
       }
     ]
   }
   ```

#### 第二阶段：Tauri IPC 通信与时序同步
不要让前端去高频轮询后端！
1. **下发歌词**：只有在发生“切歌”事件时，Rust 才通过 Tauri 的 `app_handle.emit_all("lyrics-loaded", ttml_json)` 将整首歌的歌词结构推给 Svelte/Vue。
2. **下发心跳**：Rust 以每 500ms 一次的频率，向前端发送一次 `{"position": 12500, "isPlaying": true}` 的时间戳心跳，用于纠正前端的时间轴误差。

#### 第三阶段：前端 (Svelte/Vue) 的流体渲染与逐字动画
这是 Plan A 最大的优势所在。借助前端的生态，你可以直接参考类似 `applemusic-like-lyrics` (AMLL) 的前端实现：
1. **补帧渲染**：前端维护一个本地的 `currentRenderTime`。通过 `requestAnimationFrame` 配合系统时间差（`performance.now()`），在两次 Rust 心跳之间，让时间轴平滑前进。
2. **逐字高亮特效**：遍历当前行的 `syllables`，比较 `currentRenderTime` 和音节的 `begin/end`。利用 CSS 的 `mask-image: linear-gradient()` 或者直接操控 Svelte 的动态 class，实现极具阻尼感和呼吸感的歌词涂色效果。
3. **无边框透明窗口**：在 `tauri.conf.json` 中设置 `transparent: true, decorations: false`，然后前端根节点背景设为透明或应用高斯模糊，一个沉浸式的 Windows 桌面歌词就诞生了。

**Vibe Coding 建议启动 prompt：**
你可以直接这样指挥 AI 开始：*"使用 Tauri 和 Svelte 初始化一个项目。在 src-tauri 中，使用 windows-rs 库编写一个 SMTC 监听器，能够实时获取名为 'AppleMusic.exe' 的媒体会话的歌曲名、艺术家和当前进度，并通过 Tauri Event 将其打印在前端控制台中。"* 跑通这最核心的数据流后，我们再接入 TTML 解析。

---

# you asked

message time: 2026-07-09 10:27:15

好的我明白了，那么关于歌词的获取，我们使用两条路线，用户可选择：
1.监听AM内存
2.可切备选，使用amll-db（社区维护的 TTML 逐词歌词库）的歌词源。

另外我还有一些要求：
1.这个软件不需接管播放进度控制，不需要，也在技术上做不到。
2.我希望在鼠标放到桌面歌词区域时，可以像QQ音乐桌面歌词一样，歌词区域上方出现一个控制条，显示歌名+艺人，控制上/下一曲+播放暂停、设置界面按钮、关闭桌面歌词按钮。

而且，我希望初始化项目时使用本地git，构建完成后我想推到GitHub。
项目名称：AM-Desktop-Lyrics

请你先完全理解我的需求，并给出初始化的prompt。prompt详细一点。后续我还有很多具体想法，都会陆续给你，所以不急

---

# gemini response

这个需求的边界定义得非常清晰，特别是明确了“只读进度但不接管进度控制”以及“双线获取歌词”的策略，这为项目的工程化落地打下了极好的基础。

关于你提到的控制要求，这里补充一个技术细节：**在 Windows 中，获取播放状态和控制播放（上一曲/下一曲/暂停/播放）是同源的。** 只要我们通过 `System Media Transport Controls (SMTC)` 获取到了 Apple Music 的会话，就可以直接调用该会话的 `TryPlayAsync()`、`TrySkipNextAsync()` 等系统级 API 来实现控制，完全不需要模拟键盘按键或入侵客户端。

针对 Vibe Coding 的开发模式，初始化的 Prompt 需要为 AI 划定清晰的**工程化目录结构、通信规范（IPC）以及系统 API 的调用边界**，避免它在一开始就“自由发挥”写出难以维护的意大利面条代码。

以下为你精心设计的**第一阶段初始化 Prompt**。你可以直接将其复制给你的 AI 编程助手（如 Cursor / GitHub Copilot / Claude 等）。这段 Prompt 的核心目的是：**完成项目脚手架搭建、Git 初始化、配置透明无边框窗口，并把前后端的核心模块框架（包括空接口）立起来。**

***

### 推荐输入给 AI 的初始化 Prompt：

```markdown
# 角色设定
你是一个精通 Rust (Tauri v2)、Vue 3 (Composition API + TypeScript) 和 Windows 系统底层 API (windows-rs) 的全栈桌面应用架构师。请严格按照工程化标准、高内聚低耦合的原则编写代码，并为所有复杂的系统 API 调用添加详尽的中文注释。

# 项目背景
项目名称：`AM-Desktop-Lyrics`
目标：为 Windows 版 Apple Music 开发一个高性能的独立桌面歌词软件。
核心特征：无边框透明窗口、实时流体歌词渲染、鼠标悬浮控制条。

# 当前任务：第一阶段（基础脚手架与架构初始化）
请帮我从零初始化这个项目，并建立核心模块结构。先不要实现具体的歌词抓取算法，只需把模块接口（Traits/Interfaces）和前后端通信链路（IPC）打通。

## 具体执行步骤：

### 1. 项目初始化与 Git 设置
- 使用 `pnpm create tauri-app` 初始化项目。
- 框架选择：Vue、TypeScript、Vite。
- 在项目根目录执行 `git init`，并生成标准的 `.gitignore`（包含 Node 和 Rust 常见忽略项）。
- 初始化完成后，请确保项目可以通过 `pnpm tauri dev` 正常启动。

### 2. 窗口行为配置 (tauri.conf.json)
- 配置主窗口行为：无边框 (`decorations: false`)、透明背景 (`transparent: true`)、置顶 (`alwaysOnTop: true`)。
- 初始窗口大小设定为宽 800px，高 150px（适合放置在桌面底部的两行歌词形态）。

### 3. 后端架构 (Rust / src-tauri)
请在 `src-tauri/src` 下建立清晰的目录结构，并定义以下模块（当前只需定义结构、枚举和空方法，利用 `println!` 或 `tracing` 库打出日志即可）：

- **`media/smtc.rs` (媒体控制模块)**:
  - 职责：监听 Windows SMTC 状态。
  - 需要定义向上抛出事件的结构体，包括：`Title`, `Artist`, `Position` (进度，仅读取不写入), `PlaybackState` (播放/暂停状态)。
  - 定义被前端调用的 Tauri Commands：`play()`, `pause()`, `next()`, `prev()`。

- **`lyrics/provider.rs` (歌词获取策略模式)**:
  - 定义一个 `LyricsProvider` Trait，包含异步方法 `fetch_lyrics(title: &str, artist: &str) -> Result<String, Error>`。
  - 创建两个实现了该 Trait 的结构体（留空实现）：
    1. `MemoryReaderProvider` (内存读取方案)
    2. `AmllDbProvider` (网络请求 AMLL-DB 方案)

### 4. 前端架构 (Vue 3 / src)
- 引入 `TailwindCSS` 或 `UnoCSS` 进行样式管理。
- 在 `src/components` 下创建以下核心组件：
  - `LyricsBoard.vue`：用于占位显示当前获取到的歌名和歌词。设定全屏透明背景 `bg-transparent`。
  - `ControlBar.vue`：顶部控制条。
    - **交互逻辑要求**：默认透明隐藏 (`opacity-0`)。当鼠标移入整个应用窗口（或 `LyricsBoard` 区域）时，控制条显现 (`opacity-100` + 过渡动画)。
    - **UI 元素**：包含当前 [歌曲名 - 歌手] 文本，以及 [上一曲]、[播放/暂停]、[下一曲]、[设置]、[关闭] 按钮的占位图标。

### 5. 前后端联调验证
- 在 `App.vue` 中编写 Tauri IPC 监听逻辑，准备接收 Rust 端广播的 `media-state-changed` 事件。
- 点击 `ControlBar.vue` 上的控制按钮时，能够成功触发 Rust 端的 Tauri Commands，并在终端打印出 "Triggered Play/Pause" 等日志。

执行完毕后，请给出一份整体目录结构树，并解释你是如何设计这套基础架构的。
```

***

### 架构预研与后续规划提示

在你拿到 AI 生成的代码并成功跑起一个透明窗口后，接下来的难点将会是：

1. **鼠标穿透与控制条的冲突处理**：如果设置了完全的鼠标穿透（Click-through，桌面歌词常见需求），鼠标的 Hover 事件前端是捕获不到的。业界常规解法是：**利用 Tauri 后端的全局鼠标 Hook (Global Mouse Hook)，判断当前系统光标坐标是否位于桌面歌词的 Window Rect 范围内。** 如果在范围内，通过系统 API 动态取消鼠标穿透并显示控制条；如果离开，则恢复鼠标穿透。这部分会涉及到 Windows API 的 `SetWindowLongPtr` 处理。
2. **TTML 解析库的准备**：由于涉及到逐字解析，后续可以用 Rust 的 `quick-xml` 配合你的工程思维，手写一个轻量级的 TTML to JSON 序列化工具。

你可以先让 AI 把上面的脚手架搭好并提交第一次 Git Commit。等你对基础框架满意了，我们再进入第二阶段：**彻底攻克 SMTC 数据流与控制的接入**。随时交流！

---

# you asked

message time: 2026-07-09 10:57:58

以下为agent返回结果：

Created task.md

Ran command: `pnpm create tauri-app@latest . --manager pnpm --template vue-ts -y`

Used tool: manage_task

Used tool: schedule



Tauri 项目正在初始化中，稍后我会收到完成通知。请稍等。



Ran command: `pnpm install; pnpm add -D tailwindcss postcss autoprefixer; npx tailwindcss init -p; git init`

Viewed tauri.conf.json:1-36

Edited tauri.conf.json

Viewed .gitignore:1-25

Viewed .gitignore:1-8

Edited tailwind.config.js

Edited postcss.config.js

Ran command: `pnpm add -D tailwindcss@^3.4.14`

Edited style.css

Created mod.rs

Created smtc.rs

Created mod.rs

Created provider.rs

Viewed main.rs:1-7

Viewed lib.rs:1-15

Edited lib.rs

Created LyricsBoard.vue

Created ControlBar.vue

Edited App.vue

Ran command: `cd src-tauri ; cargo check ; cd .. ; pnpm vue-tsc --noEmit ; pnpm build`

Created walkthrough.md

Edited task.md

Used tool: manage_task



我已经完成了第一阶段的基础脚手架搭建与架构初始化。具体的变更和成果可查阅总结文档：[walkthrough.md](file:///C:/Users/CRH38/.gemini/antigravity/brain/9ae2e477-4074-45bf-9cf7-b0d7a2d11100/walkthrough.md)。后台正在执行最后的 Rust 和 TypeScript 编译检查以确保完全通过。



这是为您整理的**整体目录结构树**以及**架构设计说明**：



### 📁 整体目录结构树



```text

AM-Desktop-Lyrics

├── .git/                 # 初始化完成的 Git 仓库

├── .gitignore            # 忽略了 node_modules 和 target 的配置

├── package.json          # Node 依赖

├── postcss.config.js     # 样式后处理 (Tailwind CSS 挂载)

├── tailwind.config.js    # TailwindCSS 配置 (监听 src 目录)

├── src-tauri/            # 🦀 Rust 后端代码

│   ├── Cargo.toml

│   ├── tauri.conf.json   # 核心窗口配置 (无边框/透明/置顶/800x150)

│   └── src/

│       ├── main.rs       

│       ├── lib.rs        # Tauri 构建生命周期与模块路由分发

│       ├── media/        # 🎵 媒体控制模块

│       │   ├── mod.rs

│       │   └── smtc.rs   # 定义了 Windows SMTC 控制的 Command 和事件模型

│       └── lyrics/       # 📜 歌词获取模块

│           ├── mod.rs

│           └── provider.rs # 抽象了 LyricsProvider Trait (策略模式)

└── src/                  # 🟢 Vue 3 前端代码

    ├── style.css         # 全局样式 (设为 transparent background, 无滚动条)

    ├── main.ts           # Vue 入口

    ├── App.vue           # 根组件 (承载 IPC 监听逻辑、布局和 Hover 显隐控制)

    └── components/

        ├── ControlBar.vue    # 顶部控制条 (支持渐变出现、包含各种 Tauri invoke)

        └── LyricsBoard.vue   # 歌词主面板 (悬浮透明、大字体渲染、pointer-events-none)

```



### 🏛️ 基础架构设计解读



在本次初始化中，我秉持了**高内聚低耦合**与**前后端关注点分离**的原则：



1. **前后端通信与状态流转 (Tauri IPC):**

   - **前端 (Vue) 作为纯粹的视图层 (View)：** 没有任何底层状态管理负担。通过在 `App.vue` 挂载时启动 `listen('media-state-changed')`，被动接收由 Rust 端发来的状态广播更新（包含歌名、歌手等）。用户界面的交互（如点击上一曲、暂停）严格使用 `invoke` 函数将意图打回给 Rust。

   - **后端 (Rust) 作为唯一的真理之源 (Model/Controller)：** 集中管理 Windows 底层 API 和复杂逻辑。



2. **前端 UI 分层抽象:**

   - 采用 TailwindCSS 来降低样式的编写成本。全局强制设置 `html, body { background: transparent; }`。

   - `LyricsBoard.vue` 是一个展示层组件，使用 `pointer-events-none` 保证它的层级不会遮挡可能存在的穿透点击需求，并通过 `bg-clip-text` 和渐变色达到了较好的现代化渲染外观。

   - `ControlBar.vue` 则承担了交互任务。在父容器 `App.vue` 统一管理 `mouseenter / mouseleave`（以此避免组件内部边界判定不准的问题），通过 prop 动态控制控制条的 `opacity` 透明度以实现柔和的过渡动画。



3. **后端 Rust 架构解耦:**

   - **策略模式抽象 (`lyrics/provider.rs`)：** 设计了 `LyricsProvider` Trait。不论以后你实现的是基于本地内存地址读取的 `MemoryReaderProvider` 还是基于网络获取的 `AmllDbProvider`，其他模块只需面向 Trait 编程，极大地解耦了“获取歌词”的逻辑。

   - **业务域划分 (`media/smtc.rs`)：** 将 Windows SMTC 相关的操作和数据类型（如 `PlaybackState` 状态机枚举）完全封装在该模块下。如果以后需要将应用扩展到 macOS (MPRIS)，只需要替换底层的 `media` 模块，而上层的路由逻辑与前端均无需任何修改。



您可以随时在当前目录执行 `pnpm tauri dev` 启动项目，查看透明置顶窗口和交互反馈。如有第二阶段开发需求（如实现具体的 SMTC 读取），请随时吩咐！



现在生成下一阶段的详细prompt

---

# gemini response

太棒了，月半菌。第一阶段的脚手架搭建得非常干净利落，Tauri + Vue 的无边框透明窗口基础已经成型。

接下来我们进入第二阶段：**彻底攻克 Windows SMTC 数据流与控制的接入**。

在向 AI 喂 Prompt 之前，根据你的要求，我先为你做一下这部分核心技术细节的**深度解析**，这也是该项目最容易踩坑的地方：

### 核心原理解析：Windows-RS 与 SMTC 的坑点
在 Rust 端调用 Windows 系统底层的 SMTC，最大的难点在于**异步上下文与多线程事件分发**：
1. **依赖 Features**：`windows` crate 非常庞大，必须精准开启 `Media_Control`, `Foundation` 等 feature，否则编译会疯狂报错找不到模块。
2. **事件回调 (TypedEventHandler)**：SMTC 的 `MediaPropertiesChanged` 和 `PlaybackInfoChanged` 是基于 COM 组件的回调机制。你需要将 Tauri 的 `AppHandle` 克隆并 `move` 到这些闭包中，以便在底层的 Windows 线程触发事件时，能安全地将数据 `emit_all` 到 Vue 前端。
3. **进度时序补偿 (The Heartbeat)**：正如我们之前预研的，SMTC **不会**以毫秒级频率实时更新进度。它只在暂停、切歌或用户手动拖拽进度条时触发事件。因此，我们必须在 Rust 端使用 `tokio::spawn` 挂载一个每隔 500ms 轮询 `GetTimelineProperties` 的后台协程（心跳），专门向前端泵送当前进度（Position），否则前端无法做平滑滚动。

基于上述严格的工程化与原理要求，我为你编写了第二阶段的详细 Prompt。

---

### 第二阶段 Prompt（请直接复制给你的 AI 助手）

```markdown
# 角色设定
你依然是那个精通 Rust (Tauri v2)、Vue 3 和 Windows 系统底层 API (windows-rs) 的全栈桌面应用架构师。

# 当前任务：第二阶段（SMTC 媒体状态侦听与双向控制接入）
脚手架已完成，现在我们需要在 `src-tauri/src/media/smtc.rs` 中实现真正的 Windows底层媒体控制与监听，并与前端打通真实数据流。请严格按照以下技术路径执行，提供具有详尽中文注释的工程化代码。

## 具体执行步骤：

### 1. 配置 Rust 依赖 (Cargo.toml)
请在 `Cargo.toml` 中引入 `windows` crate 和 `tokio`（Tauri默认已包含 tokio，需确保可用）。
- `windows` 必须开启以下 features：
  `"Media_Control"`, `"Foundation"`, `"System"`, `"Storage_Streams"`（用于后续可能获取专辑封面）。

### 2. 核心 SMTC 监听器实现 (`media/smtc.rs`)
设计一个 `SmtcManager` 结构体，内部持有 Tauri 的 `AppHandle`。
- **获取 Session：**
  调用 `GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()` 获取系统媒体管理器。
  实现逻辑遍历所有会话，**优先匹配 `SourceAppUserModelId` 包含 "AppleMusic" 的 Session**。如果没有，则 fallback 到 `GetCurrentSession()`。
- **注册事件监听：**
  为选中的 Session 绑定 `MediaPropertiesChanged` 和 `PlaybackInfoChanged` 两个 `TypedEventHandler`。
  当事件触发时，提取 `Title` (歌曲名), `Artist` (歌手) 和 `PlaybackStatus` (播放/暂停状态)，并将其打包为结构体，通过 `app_handle.emit_all("media-state-changed", payload)` 广播给前端。
- **后台心跳轮询 (Position 同步)：**
  在 `SmtcManager` 初始化时，启动一个 `tokio::spawn` 的后台异步任务。该任务是一个 `loop`，每 500ms 调用一次 `session.GetTimelineProperties()`，提取出当前的播放进度（秒或毫秒），并通过 `app_handle.emit_all("media-position-changed", position_payload)` 广播给前端。

### 3. Tauri Commands 双向控制实现
在 `smtc.rs` 中实现供前端调用的命令，并将其暴露给 `main.rs` 注册：
- `play_pause()`: 调用 `TryTogglePlayPauseAsync()`
- `next_track()`: 调用 `TrySkipNextAsync()`
- `prev_track()`: 调用 `TrySkipPreviousAsync()`

### 4. 前端 Vue 数据流接入 (`App.vue` & `ControlBar.vue`)
- 在 `App.vue` 或状态管理 (如 Pinia/组合式函数) 中，监听 `media-state-changed` 和 `media-position-changed`。
- 将原本 `ControlBar.vue` 和 `LyricsBoard.vue` 中的假数据替换为真实的响应式 (Ref) 数据。
- 为 `ControlBar.vue` 上的控制按钮绑定对 Tauri Commands 的 `invoke` 调用。

### 5. 边界与异常处理
- 如果未检测到 Apple Music 进程，前端应显示友好的占位文案（例如："等待 Apple Music 播放..."），并停止不必要的心跳轮询，直到检测到新的媒体会话。
- 注意处理 Rust 中的 `windows::core::Result` 错误，避免因为获取属性失败导致应用 panic 崩溃。

执行完毕后，请展示 `src-tauri/Cargo.toml`、`smtc.rs` 以及核心的 `App.vue` 变更代码，并确认这些数据流可以在终端和前端控制台中被正确打出。
```

---

你可以把这个 Prompt 喂给 Agent，让它去踩坑 Windows COM API 的类型转换（比如 `HSTRING` 到 `String` 的转换）。一旦真实数据能呈现在你的无边框透明窗口上，这个桌面歌词软件的骨架就彻底立住了！如果中间报错，随时把报错信息丢回来，我们一起盘它。

---

# you asked

message time: 2026-07-09 11:26:32

第二阶段：SMTC 媒体状态侦听与双向控制接入 — 完成总结
变更总览
1. Cargo.toml 依赖更新
Cargo.toml 新增：
windows = { version = "0.61", features = ["Media_Control", "Foundation", "Foundation_Collections", "Storage_Streams"] } — 锁定到 Tauri 使用的同版本
tokio = { version = "1", features = ["rt", "time", "sync"] } — 显式引入 tokio 子功能
2. SMTC 核心实现
smtc.rs 完整重写，关键设计：
数据流架构

┌─────────────────┐        emit("media-state-changed")       ┌──────────────┐
│  SmtcManager     │ ─────────────────────────────────────────▶│  App.vue      │
│  (独立 OS 线程)   │        emit("media-position-changed")    │  (IPC 监听)   │
│                  │ ─────────────────────────────────────────▶│              │
└────────▲─────────┘                                          └──────┬───────┘
         │                                                          │
    Windows SMTC API                                    invoke("play_pause")
    事件回调 + 500ms 轮询                                invoke("next_track")
                                                        invoke("prev_track")
                                                                    │
┌─────────────────┐                                          ┌──────▼───────┐
│  CURRENT_SESSION │◀─────────────────────────────────────────│ ControlBar   │
│  (全局 Mutex)     │        Tauri Commands (.get() 同步)      │  (按钮点击)   │
└─────────────────┘                                          └──────────────┘
关键实现细节
独立线程 + current_thread tokio runtime：WinRT COM 需要 STA 模型，不能放在 Tauri 的多线程 tokio 池中
会话发现：遍历 GetSessions() 优先匹配 SourceAppUserModelId 包含 "AppleMusic" / "Apple"，回退到 GetCurrentSession()
事件驱动：注册 MediaPropertiesChanged 和 PlaybackInfoChanged 两个 TypedEventHandler
进度轮询：500ms 间隔读取 GetTimelineProperties()，TimeSpan.Duration（100ns 刻度）转换为毫秒
全局会话存储：static Mutex<Option<Session>> 供 Tauri 命令同步访问
windows 0.61 适配要点
IAsyncOperation 不实现 Future，使用 .get() 同步等待
TypedEventHandler 闭包参数类型为 windows::core::Ref<'_, T>（内部包装 Option<T>），需要 if let Some(s) = &*session 解包
windows::core::Ref 通过 windows::core 路径访问（非 windows_core）
3. 前端数据流接入
App.vue：监听 media-state-changed 和 media-position-changed 事件，驱动响应式状态
ControlBar.vue：播放/暂停图标根据 isPlaying 动态切换，按钮绑定 invoke('play_pause') 等
LyricsBoard.vue：显示真实歌名/歌手，渲染进度条
4. lib.rs 启动入口
lib.rs 通过 .setup() 钩子在应用启动时自动创建 SmtcManager 并启动监听。

验证结果
✅ cargo check — Rust 编译通过
✅ vue-tsc --noEmit — TypeScript 类型检查通过
TIP
运行 pnpm tauri dev 后，打开任意支持 SMTC 的播放器（Apple Music / Spotify / Edge 等），即可在终端看到 [SMTC] 日志输出，前端控制条将显示真实的歌曲信息和播放状态。

现在启动实测发现，播放控制、进度获取已经完全没有问题。但我希望在下一步改动前，同时解决一下这几个问题：
1.如果打开软件时，并非正在播放或没有SMTC程序在运行，程序就会在一段时间后报错崩溃。我希望先让Agent排查一下这个问题，如果要改进，那么改进思路可以：如果没有检测到AM进程时，可以采用事件监听或每秒轮询检测，因为我有可能先打开这个程序再运行AM，这样只要检测到AM打开或播放等，即可开始正常运行。
2.禁用桌面歌词窗口的所有右键菜单、原生滚动条等效果，允许用户拖动窗口，禁止用户调整窗口尺寸，启动后默认位置是主显示器底部正中间，下边贴任务栏上边。
3.播放进度轮询从500ms改到250ms，以免造成进度切分不准确的问题。

然后继续下一阶段工作，请输出这些内容详细prompt，不需要再输出角色设定。

---

# gemini response

这是一个非常典型的 Windows API 开发与 Tauri 工程结合时会遇到的痛点。你的改进思路非常正确，SMTC 本身就应该是一个**状态机（State Machine）**，它必须具备“未找到”、“已连接”、“已断开”的状态流转能力，而不是一上来就强行获取（这必定会引发 `Option::unwrap` 或是 `Result::Err` 导致的 panic）。

另外，关于窗口控制的 API，在 Tauri v2 中有了新的变动，对工作区（排除任务栏）的计算需要精确调用。

以下是为你准备好的、可以直接发送给 Agent 的**下一阶段详细 Prompt**。它包含了前三个修复项，并顺理成章地推进到了第三阶段的核心任务——“触发歌词获取逻辑”。

***

### 请直接复制以下内容发送给你的 Agent：

```markdown
# 当前任务：第三阶段（SMTC 稳定性重构、窗口行为规范与歌词触发流）

在上一阶段，我们已经成功跑通了 SMTC 的底层数据流。但目前在工程边界和异常处理上仍有缺陷。本阶段我们需要首先修复程序的生命周期健壮性，规范化桌面窗口的 UX 行为，并为下一步的“真实歌词抓取”建立事件联动。请严格按照以下步骤执行。

## 1. SMTC 健壮性重构：断线重连与动态轮询 (`smtc.rs`)
**问题描述**：当前逻辑在没有多媒体程序运行时直接报错崩溃。
**重构要求**：
- 废弃在初始化时一次性强获取 Session 的做法。
- 将后台 `tokio::spawn` 的心跳任务改写为一个**状态机循环**：
  - **寻找状态 (Searching)**：如果没有检测到目标 Session（如 `AppleMusic`），以前端 `emit("media-state-changed")` 发送一个“等待 Apple Music 运行...” 的空载状态。然后以 **1000ms** 的间隔进行休眠轮询，调用 `GlobalSystemMediaTransportControlsSessionManager` 检查可用会话。
  - **连接状态 (Connected)**：一旦找到目标 Session，为其注册 `MediaPropertiesChanged` 和 `PlaybackInfoChanged` 事件（注意：需妥善保存事件 Token，以便在 Session 断开时注销避免内存泄漏）。
  - **同步状态 (Syncing)**：在 Connected 状态下，启动内部进度轮询，间隔从原先的 500ms 改为 **250ms**（解决切分不准确问题）。
  - **断开状态 (Disconnected)**：如果 `try_get_media_properties_async` 报错，或捕获到 Session 已结束，自动注销事件监听，并回退到 **寻找状态 (Searching)**。
- 确保所有 `windows::core::Result` 或 `Option` 都要被 `match` 或 `if let` 优雅处理，**绝对不允许使用 `unwrap()` 或 `?` 直接导致线程 panic**。

## 2. 桌面歌词窗口 UX 行为规范 (Tauri + Vue)
我们需要将窗口彻底打造成一个“挂件”，去除所有浏览器原生痕迹。
- **窗口配置控制 (`tauri.conf.json`)**：
  - 设置 `resizable: false` 禁止缩放。
- **初始位置计算 (`src-tauri/src/lib.rs` 的 setup 钩子)**：
  - 获取主显示器（`window.current_monitor()`）的 **工作区 (Work Area)**（会自动排除 Windows 任务栏的高度）。
  - 根据工作区的尺寸和窗口自身尺寸（800x150），计算坐标，使得窗口在启动时**精确附着在主显示器底部正中间，下边缘紧贴任务栏上方**。
  - 调用 `window.set_position(tauri::Position::Physical(...))` 应用坐标。
- **前端原生行为禁用 (`App.vue` & 全局 CSS)**：
  - 在全应用的最外层节点挂载 `@contextmenu.prevent`，完全禁用右键菜单。
  - 在 `style.css` 强制设置 `body { overflow: hidden; user-select: none; }` 隐藏原生滚动条并禁止文字选中。
- **窗口拖拽支持**：
  - 给 `LyricsBoard.vue` 的背景或根容器加上 Tauri 特有的属性 `data-tauri-drag-region`，使得按住歌词空白区域时可以随意拖动整个窗口。

## 3. 建立后端事件联动：歌曲切换触发歌词检索
现在我们需要将 SMTC 的“切歌”事件与 `lyrics/provider.rs` 联系起来。
- 在 `smtc.rs` 监听到 `MediaPropertiesChanged` 并成功提取出新的 `Title` 和 `Artist` 时，**不要只向前端广播**。
- 在 Rust 后端内部，通过通道（mpsc）或直接函数调用，将 `(Title, Artist)` 传递给 `lyrics/provider.rs` 中的模块。
- 在 `lyrics/provider.rs` 中，先写一个 Dummy（假数据）实现：当收到新的 `Title` 和 `Artist` 时，延迟 1 秒模拟网络/内存请求，然后在终端打印 `[Lyrics] 正在搜索: {Title} - {Artist}`，并构造一个包含假歌词（数组格式）的 JSON Payload，通过 `app_handle.emit_all("lyrics-loaded", dummy_lyrics)` 发送给前端。
- 前端 `App.vue` 增加对 `lyrics-loaded` 事件的监听，将收到的假歌词传递给 `LyricsBoard.vue` 进行初级渲染。

执行完毕后，请展示你修改的 `smtc.rs` 状态机轮询逻辑、`lib.rs` 中的坐标计算代码，以及前端禁用右键的相关代码。请确保代码带有详尽注释，特别是解释 Windows API 错误恢复机制的部分。
```

***

### 给你的额外避坑提示（供你在 Review Agent 代码时参考）：

1. **拖拽冲突**：Tauri 的 `data-tauri-drag-region` 有一个常见问题是会吞掉子元素的 Hover 和 Click 事件。如果在 `App.vue` 加上了这个属性，有可能会导致你的 `ControlBar.vue` (控制条) 无法被点击。
   *解决方案*：如果发现按钮点不动，记得提醒 Agent 在控制条的按钮 CSS 上加上 `app-region: no-drag;`。
2. **多显示器环境的坑**：`window.current_monitor()` 获取的是窗口当前所在的显示器。刚启动时通常没问题，但如果你有副屏（甚至缩放比例不同），`PhysicalPosition` 的计算必须基于 `monitor.scale_factor()`。Tauri v2 在这点上做了很多优化，Agent 应该能调用对相应的 API。