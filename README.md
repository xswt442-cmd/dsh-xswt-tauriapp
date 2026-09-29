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

DeepSeek Harness 的轻量 Tauri 桌面外壳，也是 dsh 的 **desktop harness / runtime supervisor**：Tauri 负责 dsh 的外围，包括窗口、进程、启动与复用、会话交接、菜单与托盘、快捷键、更新、外链与故障恢复；页面内容仍由 dsh 自己负责。外壳不修改 dsh 源码，不向 dsh 页面注入脚本、不读取它的 DOM、不覆盖它的样式或界面，也不介入 dsh 的会话、沙箱与权限模型。

## 功能

- **服务复用与启动**：在 `3080`–`3129` 中复用已在运行的 `dsh web`，否则在首个空闲端口启动一个；服务进程独立于外壳，关闭窗口不会停止它。
- **启动 token 握手与会话交接**：握手在 Rust 侧完成并校验，启动 token 不进入任何页面的 URL。
- **启动时选择端口**：默认值取扫描结果或上次手输的端口，也可以输入任意端口；可用性当场判定。
- **更新提示**：dsh 的更新按正式版 / RC / Alpha 分栏展示，外壳自身的更新按平台挑选安装包并校验发布页的 `SHA256SUMS`；两者各自记录「不再提示」。
- **系统集成**：macOS 使用系统菜单，Windows / Linux 使用托盘菜单；`Ctrl/Cmd+R`、`Ctrl/Cmd+=` `-` `0` 与 `F12` 只在自身窗口获得焦点时注册。
- **站外链接交给系统默认程序打开**，dsh 窗口保持原页面。

## 平台与兼容性

| 项 | 状态 |
|---|---|
| Linux（deb / rpm / AppImage） | 支持，由 CI 产出；会话交接与首次导航已实测 |
| Windows（NSIS 安装包） | 支持；安装包与 GUI 已在真机日常使用中验证，交接与首次导航均已实测 |
| macOS（dmg） | 构建已接入，未签名；GUI 未实测，首次导航能否带上 `SameSite=Strict` 的会话 cookie，取决于 WebView 是否把宿主发起、没有来源页面的导航判为同站 |
| WSLg | 可运行；WebKitGTK 的 GPU 直通不稳定，需设 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 与 `WEBKIT_DISABLE_DMABUF_RENDERER=1`；没有状态栏宿主，托盘图标无法显示，快捷键需要使用 X11 后端；窗口按 scale 1 渲染，在 125% / 150% 的显示器上比原生应用小，缩放见 `DSH_SHELL_ZOOM` |
| dsh | 下限是 `0.1.5-rc.1`；CI 的 boot-check 覆盖 `0.1.5-rc.1`、`latest` 与 `0.2.0-rc.1` 三条线；安装旧版本时 npm 按各组件自身的 caret 范围解析，`0.1.5-rc.1` 一条用 `--before 2026-09-11T00:00:00Z` 固定依赖树 |

## 获取

### 从插件市场安装

插件市场（[awesome-dsh-plugin](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin)）的条目指向本仓库每个 release 附带的 `dsh-xswt-tauriapp-plugin.tgz`。它是下载并校验安装包的引导程序，外壳本体仍要从同一个 release 安装（源码见 [`plugins/dsh-desktop-app/`](plugins/dsh-desktop-app/README.md)）：

```sh
dsh plugin --profile web add https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
```

装上后**首次**启动 dsh 时，引导读取本仓库最新 release，按平台挑选安装包，先取 `SHA256SUMS`、校验通过才写盘，再交给系统安装器。已经装好外壳的机器，以及没有桌面会话的机器（`CI`，或 Linux 上没有 `DISPLAY` 与 `WAYLAND_DISPLAY`），只会收到一句说明。引导读取这些变量：

| 变量 | 作用 |
|---|---|
| `DSH_TAURIAPP_MODE` | 只认 `auto`、`notice`、`off`，写成其他值等于没有设置，仍按插件配置决定 |
| `DSH_TAURIAPP_FORCE` | 值为 `1` 时，在已经提示过一次之后仍然重新下载 |
| `DSH_TAURIAPP_NO_OPEN` | 值为 `1` 时，停在即将交给系统安装器之前，只打印该执行的命令 |
| `DSH_TAURIAPP_RELEASES_API` | 非空时覆盖发布接口地址 |
| `DSH_TAURIAPP_INSTALL_DIRS` | 以路径列表替换「已经安装」探测所查找的目录 |

状态文件与下载的安装包都放在 `$DSH_HOME` 下，该变量非空即按字面采用，与外壳使用同一条判据。引导不静默安装，也不导入任何 harness API，因此不会成为 dsh 启动失败的原因。

### 使用 Release 产物

每个 release 附带 Windows 安装包、macOS dmg（x64 与 aarch64）、deb、rpm 与 AppImage，以及 `SHA256SUMS`。deb 依赖 `libwebkit2gtk-4.1-0`、`libgtk-3-0` 与 `libayatana-appindicator3-1`（最后一项是托盘所需；Ubuntu 24.04 起 GTK3 的包名是 `libgtk-3-0t64`，它 `Provides: libgtk-3-0`，因此该依赖仍然被满足）：

```sh
sudo apt install ./dsh-xswt-tauriapp_*_amd64.deb
sudo dnf install ./dsh-xswt-tauriapp-*.x86_64.rpm
```

外壳自身的更新在 Linux 只自动挑选 `.deb`；`.rpm` 与 AppImage 不参与自动更新。

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
| `DSH_HOME` | DSH 主目录：非空时按字面采用，那个目录还不存在也一样；为空才回落到 `~/.dsh`（家目录取 `HOME`，Windows 上取 `USERPROFILE`） |
| `DSH_BIN` | 非空时作为 `dsh` 的 `lib/bin.js` 的首选；它不存在时仍会继续尝试 `$DSH_HOME/profiles`、`npm_config_prefix` 与 `PATH` 里的候选 |
| `DSH_NODE_BIN` | 非空时作为 `node` 可执行文件的候选，排在 `PATH` 之前；但拥有当前 dsh 安装的那个 node 若存在，会先于它被采用 |
| `DSH_TAURI_REGISTRY` | 非空时覆盖 dsh 的版本查询地址，否则用 npm registry |
| `DSH_SHELL_RELEASES_API` | 非空时覆盖外壳自身更新读取的 GitHub Releases API 地址，否则用本仓库的 `releases/latest`；供测试与镜像使用 |
| `DSH_SHELL_ALLOW_MULTIPLE` | 只要被设置就允许同时运行多个外壳，空值与 `0` 同样算设置；默认第二次启动只把已在运行的那个提到前面 |
| `DSH_SHELL_DEBUG` | 只要被设置就把交接、导航与更新检查的日志写到 stderr，空值也算设置；debug 构建无需设置，总是输出 |
| `DSH_SHELL_DEVTOOLS` | 只要被设置就在发布构建里提供开发者工具，空值也算设置；debug 构建无需设置。未开启时两个窗口都不带检查器，菜单里也没有那一项，`F12` 仍被注册但按下去不开任何窗口 |
| `DSH_SHELL_ZOOM` | dsh 窗口的初始缩放因子，优先于记忆值：能解析成大于 `0` 的数字才生效（空值、`0` 与拼错的值一律忽略，回落记忆值），过界的值收敛到 0.3–3.0 |
| `DSH_SHELL_WAYLAND` | 在 Linux 上只要被设置就不切换到 X11 后端，写成 `0` 同样是退出，习惯上置 `1`；显式设置了 `GDK_BACKEND` 时外壳同样不干预 |

## 工作原理

| 环节 | 行为 |
|---|---|
| 端口段 | `3080`–`3129`；候选取自 `$DSH_HOME/launcher/logs/server-<port>.out.log` 的文件名，没有日志就没有启动 token，超过 90 天的旧日志不算候选 |
| 复用 | 读取该日志尾部的启动 token，`GET /?token=…` 取得 303 与会话 cookie，再带着 cookie 重发一次并校验页面标记；未启用 dsh 认证的服务以 `GET /` 的 200 采纳 |
| 启动 | `node <dsh>/lib/bin.js web --port <p> --no-open`，独立进程组，输出追加到上述日志 |
| 端口选择 | 已发现服务时预填其端口，确认即复用；否则默认取端口段内第一个空闲端口，只有手输过的端口会被记住；弹窗另列至多 6 个有日志的端口及各自的判定；`1024` 以下拒绝；本机无法进入其会话的 dsh 会单独说明，不会报成端口被占用 |
| 窗口 | `bootstrap` 是本地源、唯一持有 IPC capability；`dsh` 是远端源，不获得外壳 IPC |
| 交接 | dsh 会话 cookie 带 `SameSite=Strict`，握手与 cookie 写入由 Rust 先完成，之后再创建 dsh 窗口；该窗口的首次导航由宿主发起，不构成跨站请求 |
| 外链 | 仅 `http:`、`https:`、`mailto:` 交还系统默认程序；`file:` 与解析失败的文本被拒绝，原因写入外壳日志（`DSH_SHELL_DEBUG`） |
| 更新检查 | dsh 查 npm、外壳查本仓库 GitHub Releases，两条路径并行且互不比较版本；检查失败（断网、接口限流、尚无发布）时不提示 |
| 更新候选 | 启动弹窗只取稳定度不低于已装版本的通道，通道由版本字符串判定而不使用 npm dist-tag；「不再提示此版本」按版本记录在应用配置目录，与 dsh 的忽略列表互不影响 |
| 安装包 | 交给系统之前先比对 `SHA256SUMS`：校验不通过就报出两个哈希且不落盘，发布里没有校验文件就不下载，该发布没有本平台可用的安装包时提示改为打开发布页 |
| 菜单与快捷键 | macOS 使用原生系统菜单，文本框快捷键依赖其中的「编辑」菜单；Windows 与 Linux 使用托盘菜单，不挂永久菜单栏。快捷键只在外壳自身的窗口获得焦点期间注册，缩放走原生 `set_zoom` 而不注入页面脚本；Linux 的按键抓取经由 X11，原生 Wayland 窗口的按键不经过 X 服务器，因此在有 `DISPLAY` 时默认使用 X11 后端 |
| 记忆 | 手输过的端口、缩放因子、窗口大小与位置存于应用配置目录；记忆的位置仅在当前某块显示器能容纳它时采用 |

## 开发与验证

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node --test scripts/changelog.test.mjs
node --test plugins/dsh-desktop-app/test/plugin.test.js
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` 不依赖 GUI 工具链，可以在缺少 `libwebkit2gtk` 的环境中单独构建与测试。`examples/launch.rs` 以与外壳相同的代码路径启动真实服务，输出 `url=` 与 `cookie=`，compat CI 用它验证握手并确认启动 token 不进入 URL。它输出的 cookie 是真实会话凭据，写进报告前需要涂掉。`plugins/dsh-desktop-app/` 的市场引导插件只使用 Node 内置模块，测试运行在本机回环的替身发布服务器上，不访问 GitHub，也不打开安装包。

## License

[MIT](./LICENSE)
