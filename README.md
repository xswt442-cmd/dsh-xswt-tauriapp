# dsh-xswt-tauriapp

**鉴于官方桌面版的推出，本仓库于 2026-09-30 起停止维护。**

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
[![DSH](https://img.shields.io/badge/DSH-desktop%20harness-4d6bfe)](https://github.com/deepseek-ai/deepseek-harness)
[![release](https://img.shields.io/github/v/release/xswt442-cmd/dsh-xswt-tauriapp?label=release&color=2ea043)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest)
[![DSH compatibility](https://img.shields.io/badge/DSH-%3E%3D0.1.5--rc.1-4d6bfe)](#平台与兼容性)
[![platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078d4)](#平台与兼容性)
[![runtime](https://img.shields.io/badge/runtime-system%20WebView-8957e5)](#平台与兼容性)
[![compat](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml/badge.svg)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml)
[![downloads](https://img.shields.io/github/downloads/xswt442-cmd/dsh-xswt-tauriapp/total?label=downloads)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases)

DeepSeek Harness (Web) 的轻量 Tauri 桌面外壳，也是 dsh 的 **desktop harness / runtime supervisor**：窗口、服务进程、会话交接、菜单与快捷键、更新与外链由外壳负责，页面内容由 dsh 自己负责。外壳不修改 dsh，也不向它的页面注入脚本。

## 功能

- **服务复用与启动**：复用 `3080`–`3129` 中已在运行的 `dsh web`，否则在首个空闲端口启动一个；服务进程独立于外壳，关闭窗口不会停止它。
- **启动 token 握手与会话交接**：token 只在 Rust 侧使用，不进入任何页面的 URL。
- **启动时选择端口**：默认值来自端口扫描或上次手输的端口，也可以输入其他端口。
- **更新提示**：dsh 的更新按正式版 / RC / Alpha 分栏展示，外壳自身的更新按平台挑选安装包并校验 `SHA256SUMS`；两者的「不再提示」各自记录。
- **系统集成**：macOS 使用系统菜单，Windows / Linux 使用托盘菜单；`Ctrl/Cmd+R`、缩放与 `F12` 只在外壳窗口获得焦点时注册。
- **站外链接**交给系统默认程序打开，dsh 窗口保持原页面。

## 平台与兼容性

| 项 | 状态 |
|---|---|
| Linux（deb / rpm / AppImage） | 支持 |
| WSLg | 支持，偶发性运行不稳 |
| Windows（NSIS 安装包） | 支持 |
| macOS（dmg） | 构建已接入，未签名，GUI 未实测 |
| dsh | 下限 `0.1.5-rc.1` |

WSLg 需要设置 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 与 `WEBKIT_DISABLE_DMABUF_RENDERER=1`。

## 获取

### 从插件市场安装

插件市场（[awesome-dsh-plugin](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin)）条目的 tarball 是本仓库每个 release 附带的 `dsh-xswt-tauriapp-plugin.tgz`。它是下载并校验安装包的引导程序，外壳本体仍要从同一个 release 安装（源码见 [`plugins/dsh-desktop-app/`](plugins/dsh-desktop-app/README.md)）：

```sh
dsh plugin --profile web add https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
```

首次启动 dsh 时，引导读取最新 release，按平台挑选安装包，校验 `SHA256SUMS` 通过后写盘，再交给系统安装器。已经装好外壳的机器和没有桌面会话的机器只会收到一句说明。引导读取这些变量：

| 变量 | 作用 |
|---|---|
| `DSH_TAURIAPP_MODE` | 取值 `auto`、`notice`、`off`，其他值按插件配置决定 |
| `DSH_TAURIAPP_FORCE` | `1` 时在已提示过一次之后仍然重新下载 |
| `DSH_TAURIAPP_NO_OPEN` | `1` 时不交给系统安装器，只打印该执行的命令 |
| `DSH_TAURIAPP_RELEASES_API` | 覆盖发布接口地址 |
| `DSH_TAURIAPP_INSTALL_DIRS` | 替换「已经安装」探测所查找的目录 |

状态文件与下载的安装包放在 `$DSH_HOME` 下。引导不静默安装，也不导入任何 harness API。

### 使用 Release 产物

每个 release 附带 Windows 安装包、macOS dmg（x64 与 aarch64）、deb、rpm、AppImage 与 `SHA256SUMS`。deb 依赖 `libwebkit2gtk-4.1-0`、`libgtk-3-0` 与 `libayatana-appindicator3-1`（最后一项是托盘所需，Ubuntu 24.04 起 `libgtk-3-0` 由 `libgtk-3-0t64` 提供）：

```sh
sudo apt install ./dsh-xswt-tauriapp_*_amd64.deb
sudo dnf install ./dsh-xswt-tauriapp-*.x86_64.rpm
```

外壳自动更新在 Linux 只挑选 `.deb`。

### 从源码构建

需要 Rust、Node 与 Tauri 的 Linux 系统依赖：

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev rpm
pnpm install
pnpm tauri build --bundles deb,rpm,appimage
```

## 使用

```sh
./src-tauri/target/release/dsh-xswt-tauriapp   # Windows 上是 dsh-xswt-tauriapp.exe
```

外壳先显示 `bootstrap` 页面，同时完成服务发现与更新检查，随后打开 dsh 窗口。可用环境变量：

| 变量 | 作用 |
|---|---|
| `DSH_HOME` | DSH 主目录，非空即按字面采用，目录还不存在也一样；为空时回落 `~/.dsh` |
| `DSH_BIN` | dsh 的 `lib/bin.js` 首选路径；未命中时继续按 `$DSH_HOME/profiles`、`npm_config_prefix` 与 `PATH` 查找 |
| `DSH_NODE_BIN` | 指定 `node`；拥有已装 dsh 的那个 node 优先 |
| `DSH_TAURI_REGISTRY` | dsh 的版本查询地址，默认 npm registry |
| `DSH_SHELL_RELEASES_API` | 外壳更新检查读取的 Releases 地址，默认本仓库的 `releases/latest` |
| `DSH_SHELL_ALLOW_MULTIPLE` | 设置即允许多个外壳同时运行，默认第二次启动只前置已在运行的那个 |
| `DSH_SHELL_DEBUG` | 设置即把交接、导航与更新检查的日志写到 stderr |
| `DSH_SHELL_DEVTOOLS` | 设置即在发布构建里提供开发者工具；未设置时两个窗口都没有检查器 |
| `DSH_SHELL_ZOOM` | dsh 窗口的初始缩放因子，优先于记忆值，取 0.3–3.0 |
| `DSH_SHELL_WAYLAND` | Linux 上设置即不切换到 X11 后端，已显式设置 `GDK_BACKEND` 时不干预 |

「设置即生效」的变量按是否存在判断，写成 `0` 或空值同样算设置。debug 构建总是写日志并带开发者工具。

## 工作原理

| 环节 | 行为 |
|---|---|
| 端口段 | 默认 `3080`–`3129`；端口候选取自 `$DSH_HOME/launcher/logs/server-<port>.out.log` 的文件名，只认 90 天内的日志 |
| 复用 | `GET /?token=…` 取得 303 与会话 cookie，带着 cookie 重发一次并校验页面标记；未启用认证的服务以 `GET /` 的 200 采纳 |
| 启动 | `node <dsh>/lib/bin.js web --port <p> --no-open`，独立进程组，输出追加到上述日志 |
| 端口选择 | 已发现服务时预填其端口，否则默认端口段内第一个空闲端口，只有手输过的端口会被记住；弹窗列出至多 6 个有日志的端口及各自判定；`1024` 以下拒绝，无法进入会话的 dsh 单独说明 |
| 窗口 | `bootstrap` 是本地源，唯一持有 IPC capability；`dsh` 是远端源，不获得外壳 IPC |
| 交接 | dsh 的会话 cookie 是 `SameSite=Strict`，握手与 cookie 写入先完成，再创建 dsh 窗口；首次导航由宿主发起 |
| 外链 | 只把 `http:`、`https:`、`mailto:` 交给系统默认程序；`file:` 与无法解析的文本被拒绝 |
| 更新 | dsh 查 npm、外壳查本仓库 Releases，两条路径并行且互不比较版本；检查失败不提示；候选只取稳定度不低于已装版本的通道 |
| 安装包 | 先比对 `SHA256SUMS` 才交给系统：校验不通过不落盘，没有校验文件不下载，本平台无可用安装包时改为打开发布页 |
| 菜单与快捷键 | 快捷键只在外壳窗口获得焦点期间注册，缩放走原生 `set_zoom`；Linux 有 `DISPLAY` 时默认使用 X11 后端 |
| 记忆 | 手输过的端口、缩放因子、窗口大小与位置存于应用配置目录，记忆的位置不在任何现有显示器范围内时不采用 |

## 开发与验证

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node --test scripts/changelog.test.mjs
node --test plugins/dsh-desktop-app/test/plugin.test.js
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` 不依赖 GUI 工具链，可以在缺少 `libwebkit2gtk` 的环境中构建与测试。`examples/launch.rs` 走与外壳相同的代码路径启动真实服务并输出会话，compat CI 用它验证握手；它输出的 cookie 是真实会话凭据，写进报告前需要涂掉。`plugins/dsh-desktop-app/` 只使用 Node 内置模块，测试在本机回环的替身发布服务器上运行。

## License

[MIT](./LICENSE)
