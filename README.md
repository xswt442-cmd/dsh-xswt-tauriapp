# dsh-xswt-tauriapp

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
[![DSH](https://img.shields.io/badge/DSH-desktop%20harness-4d6bfe)](https://github.com/deepseek-ai/deepseek-harness)
[![release](https://img.shields.io/github/v/release/xswt442-cmd/dsh-xswt-tauriapp?label=release&color=2ea043)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest)
[![DSH compatibility](https://img.shields.io/badge/DSH-%3E%3D0.1.5--rc.1-4d6bfe)](#平台与兼容性)
[![platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078d4)](#平台与兼容性)
[![runtime](https://img.shields.io/badge/runtime-system%20WebView-8957e5)](#平台与兼容性)
[![compat](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml/badge.svg)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml)
[![downloads](https://img.shields.io/github/downloads/xswt442-cmd/dsh-xswt-tauriapp/total?label=downloads)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases)

DeepSeek Harness 的轻量 Tauri 桌面外壳，也是 dsh 的 **desktop harness / runtime supervisor**：Tauri 负责 dsh 的外围——窗口、进程、启动与复用、会话交接、菜单与托盘、快捷键、更新、外链与故障恢复；页面内容仍由 dsh 自己负责。外壳不修改 dsh 源码，不向 dsh 页面注入脚本、不读取它的 DOM、不覆盖它的样式或界面，也不介入 dsh 的会话、沙箱与权限模型。

## 功能

- **服务复用与启动**：在 `3080`–`3129` 中复用已在运行的 `dsh web`，否则在首个空闲端口启动一个；服务进程独立于外壳，关闭窗口不会停止它。
- **启动 token 握手与会话交接**：握手在 Rust 侧完成并校验，启动 token 不进入任何页面的 URL。
- **启动时选择端口**：默认值取扫描结果或上次手输的端口，也可以输入任意端口；可用性当场判定。
- **更新提示**：dsh 的更新按正式版 / RC / Alpha 分栏展示，外壳自身的更新按平台挑选安装包并校验发布页的 `SHA256SUMS`；两者各自记录「不再提示」。
- **系统集成**：macOS 使用系统菜单，Windows / Linux 使用托盘菜单；`Ctrl/Cmd+R`、`Ctrl/Cmd+=` `-` `0` 与 `F12` 只在自身窗口获得焦点时注册。
- **站外链接交给系统默认程序打开**，不接管应用窗口。

## 获取

### 从插件市场安装

插件市场（[awesome-dsh-plugin](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin)）的条目指向本仓库每个 release 附带的 `dsh-xswt-tauriapp-plugin.tgz`，它是安装引导而非安装包本体（源码见 [`plugins/dsh-desktop-app/`](plugins/dsh-desktop-app/README.md)）：

```sh
dsh plugin --profile web add https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
```

装上后**首次**启动 dsh 时，引导会读取本仓库最新 release，按平台挑选安装包，先取 `SHA256SUMS`、校验通过才写盘，再交给系统安装器。已装好外壳的机器，以及没有桌面会话的机器（CI，或 Linux 上没有 `DISPLAY`），只会看到一句说明。它不静默安装，也不导入任何 harness API，因此不会成为 dsh 启动失败的原因。

### 使用 Release 产物

每个 release 附带 Windows 安装包、macOS dmg、deb、rpm 与 AppImage，以及 `SHA256SUMS`。deb 仅依赖 `libwebkit2gtk-4.1-0` 与 `libgtk-3-0`：

```sh
sudo apt install "./dsh-xswt-tauriapp_0.0.8_amd64.deb"
```

### 从源码构建

需要 Rust、Node 与 Tauri 的 Linux 系统依赖：

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev rpm
pnpm install
pnpm tauri build --bundles deb,rpm,appimage
```

## 使用

```sh
./src-tauri/target/release/dsh-xswt-tauriapp
```

外壳先显示 `bootstrap` 页面，同时完成服务发现与更新检查，随后打开 dsh 窗口。可用环境变量：

| 变量 | 作用 |
|---|---|
| `DSH_HOME` | DSH 主目录，默认 `~/.dsh` |
| `DSH_BIN` | 直接指定 `dsh` 的 `lib/bin.js` |
| `DSH_NODE_BIN` | 直接指定 `node` 可执行文件 |
| `DSH_TAURI_REGISTRY` | 覆盖版本查询地址，默认 npm registry |
| `DSH_SHELL_DEBUG` | 非空时输出交接、导航与更新检查日志 |
| `DSH_SHELL_DEVTOOLS` | 非空时在菜单中提供开发者工具（debug 构建默认提供） |
| `DSH_SHELL_WAYLAND` | 在 Linux 上不切换到 X11 后端（见「菜单、托盘与快捷键」） |

## 工作原理

### 服务发现、握手与会话交接

| 步骤 | 行为 |
|---|---|
| 端口段 | `3080`–`3129` |
| 复用判定 | 读取 `$DSH_HOME/launcher/logs/server-<port>.out.log` 尾部的启动 token |
| 握手（Rust 侧） | `GET /?token=…` → 303 与会话 cookie → 携带 cookie 复请，校验页面含 `DeepSeek Harness` |
| 交接 | 把 cookie 写入 cookie 存储（补上服务未给出的 `Domain`），确认可读之后才创建 dsh 窗口 |
| 无认证服务 | 直接 `GET /` 返回 200 且含标记时同样采纳 |
| 启动 | `node <dsh>/lib/bin.js web --port <p> --no-open`，独立进程组，输出追加到上述日志 |

服务日志是启动 token 的唯一来源：在终端手工启动、日志未落在该路径的实例不会被识别，外壳会另起一个实例。这是 dsh 的鉴权模型决定的，外壳不做额外猜测。

### 端口选择

| 情况 | 行为 |
|---|---|
| 已发现可复用的服务 | 端口框预填它的端口，确认即复用；填别的端口会在新端口再起一个实例 |
| 没有可复用的服务 | 默认 = `3080`–`3129` 中第一个空闲端口；手输过的端口若仍空闲则优先 |
| 端口被别的程序占用 | 当场提示，确认不会继续 |
| 端口上是 dsh，但本机无法接手其会话 | 单独说明——例如在 Windows 侧启动的实例，或用了另一个 `DSH_HOME` |
| 低于 `1024` | 直接拒绝，普通用户无法绑定 |

只有**手输**过的端口会被记住并作为下次的默认值；接受灰色默认值不算选择，默认值因此继续跟随「第一个空闲端口」。

候选端口取自 `$DSH_HOME/launcher/logs/server-<port>.out.log` 的**文件名**，而不是对整个端口段做扫描：没有日志的端口没有启动 token，握手不可能完成，探测它没有意义。这也让刻意选用的非标准端口（例如 `9000`）在下次启动时仍能被发现。

### 窗口模型与首次导航

| 窗口 | 内容 | 权限 |
|---|---|---|
| `bootstrap` | 外壳自带页面：进度、更新弹窗、失败信息 | 本地源，唯一被授予 IPC capability 的窗口 |
| `dsh` | dsh 的原始界面 | 远端源，不获得外壳 IPC |

dsh 的会话 cookie 带 `SameSite=Strict`，由其他源的页面发起的导航不会携带它，这正是把外壳页面直接导航到 dsh 会停在 dsh 401 文本上的原因。外壳保留 dsh 的安全语义，只调整交接顺序：Rust 先完成握手并把 cookie 写入 cookie 存储，之后才以 `http://127.0.0.1:<port>/` 创建 dsh 窗口。该窗口的首次导航由宿主发起，没有发起者页面，因此不是跨站请求。

### 菜单、托盘与快捷键

| 平台 | 形态 |
|---|---|
| macOS | 原生系统菜单（含「编辑」菜单，文本框快捷键依赖它） |
| Windows / Linux | 托盘菜单；不挂永久菜单栏，不占用 dsh 的高度 |

Tauri 的快捷键只能挂在菜单 accelerator 上，而 Windows / Linux 上挂在窗口的菜单就是可见菜单栏。这两个平台因此改用全局快捷键，并且只在自身窗口获得焦点期间注册、失焦即注销，不会长期占用整台机器的 `Ctrl+R`。桌面环境拒绝发放全局快捷键时，外壳照常启动，只是失去快捷键。

Linux 上另有一个前提：`global-hotkey` 通过 X11 抓键，而 Wayland 原生窗口的按键不经过 X 服务器，快捷键会注册成功但永不触发。外壳因此在有 `DISPLAY` 时默认使用 X11 后端（XWayland 在所有 Wayland 桌面上都存在），设置 `DSH_SHELL_WAYLAND=1` 可退出该行为；显式设置 `GDK_BACKEND` 时外壳不干预。

缩放由 Rust 调用原生 `set_zoom` 完成：WebView 自带的缩放热键在 macOS / Linux 上依赖向页面注入 polyfill，与本项目的不注入原则冲突。

### 外壳自身的更新

与 dsh 的更新是两条独立路径：dsh 来自 npm，外壳来自本仓库的 GitHub Releases，启动时并行检查。

| 情况 | 行为 |
|---|---|
| 有新版本，且该发布带本平台可安装的安装包 | 弹窗顶部提示「下载并安装」；下载后比对 `SHA256SUMS`，通过才交给系统安装器打开 |
| 有新版本，但该发布没有本平台的安装包 | 提示改为「打开发布页」，不猜、不下载别的东西 |
| 校验不通过 | 拒绝安装并说明期望与实际哈希，**不落盘** |
| 发布里没有 `SHA256SUMS` | 直接拒绝下载未经校验的安装包 |
| 检查失败（断网、API 限流、还没有任何发布） | 静默，不提示；不确定有新版本不是打扰用户的理由 |

版本比较要求两边都能解析成 semver，并先剥离 tag 的 `v` 前缀。这里不复用 dsh 的比较函数：它的字符串回退适用于版本流，用于更新器则会把当前正在运行的版本当成新版本提示一次。

「不再提示」按外壳版本单独记录（`dismissed-shell-updates.json`），与 dsh 的忽略列表互不影响。

### dsh 自身的更新

通道由版本字符串判定，不使用 npm dist-tag——后者自身可能指向一个 RC。

| 通道 | 判定 |
|---|---|
| 正式版 | 无预发布后缀，如 `0.1.5` |
| RC | 预发布段以 `rc` 开头，如 `0.1.5-rc.2` |
| Alpha | 预发布段以 `alpha` 开头，如 `0.1.6-alpha.1` |

启动弹窗只在稳定度不低于已装版本的通道中选取候选：已装 RC 时只会被提示 RC 或正式版，Alpha 需在弹窗中主动选择。「不再提示此版本」把该版本写入应用配置目录，且仅抑制该版本。

## 平台与兼容性

| 项 | 状态 |
|---|---|
| Linux（deb / rpm / AppImage） | 支持，由 CI 产出；会话交接与首次导航已实测 |
| Windows（NSIS 安装包） | 构建已接入；核心逻辑经 CI 在 `windows-latest` 上验证，GUI 未实测 |
| macOS（dmg） | 构建已接入，未签名；GUI 未实测 |
| WSLg | 可运行；WebKitGTK 的 GPU 直通不稳，需设 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 与 `WEBKIT_DISABLE_DMABUF_RENDERER=1`。WSLg 没有状态栏宿主，托盘图标无处显示；快捷键可用，前提是走 X11 后端 |

首次导航发送 `SameSite=Strict` cookie 的行为已在 Linux / WebKitGTK 上实测确认；Windows 与 macOS 依赖各自 WebView 对无发起者导航的同站判定，尚未实测。

## 开发与验证

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node --test plugins/dsh-desktop-app/test/plugin.test.js
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` 不依赖 GUI 工具链，可在缺少 `libwebkit2gtk` 的环境中独立构建与测试。`examples/launch.rs` 以与外壳相同的代码路径启动真实服务并输出准备好的会话（`url=` 与 `cookie=`）；compat CI 用它验证握手，并确认启动 token 不进入 URL。

`plugins/dsh-desktop-app/` 的市场引导插件是零依赖的纯 Node 模块，其测试全部运行在本机回环的替身发布服务器上：不访问 GitHub，也不打开任何安装包（`DSH_TAURIAPP_NO_OPEN=1` 使它停在即将交给系统安装器的那一刻）。

## License

[MIT](./LICENSE)
