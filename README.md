# dsh-xswt-tauriapp

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
![DSH shell](https://img.shields.io/badge/DSH-shell-4d6bfe)

DeepSeek Harness 的轻量 Tauri 桌面外壳 —— 更准确地说，是 dsh 的 **desktop harness / runtime supervisor**：Tauri 负责 dsh 的外围（窗口、进程、启动与复用、会话交接、菜单/托盘/快捷键、更新、外链、故障恢复），dsh 自己负责页面内容。外壳不修改 dsh 源码，**不向 dsh 页面注入脚本、不读它的 DOM、不改它的 CSS，也不在它上面覆盖任何外壳 UI**，同样不介入 dsh 的会话、沙箱与权限模型。

## 功能

- **服务复用与启动**：扫描 3080–3129 端口段，优先复用已在运行的 `dsh web`，否则在首个空闲端口启动一个。
- **启动 token 握手与会话交接**：dsh 只把界面交给携带启动 token 的请求。握手在 Rust 侧走完并校验结果，得到的会话 cookie 交给窗口，启动 token 不进入任何页面的 URL。
- **启动时选择端口**：弹窗在版本三栏下方给出端口框，灰色数字就是默认端口（`3080`–`3129` 中第一个空闲的，手输过的端口优先）。直接确认即按默认启动，也可以自己输入任意端口；被占用的端口会当场说明，而不是等到启动失败。
- **两个窗口**：外壳自带的 `bootstrap` 页面负责进度、更新与错误；`dsh` 窗口只显示 dsh。启动过程中的任何界面都不可能盖在 dsh 上。
- **启动时检查更新**：读取 npm 上 `@deepseek-ai/dsh` 的已发布版本，按正式版 / RC / Alpha 三栏展示，可选定任一版本更新并重启。
- **不再提示此版本**：勾选后该版本不再触发启动弹窗；更新的版本出现时仍会提示。
- **菜单、托盘与快捷键**：macOS 用原生系统菜单；Windows / Linux 用托盘菜单，不挂永久菜单栏。`Ctrl/Cmd+R` 重新载入、`Ctrl/Cmd+=` `-` `0` 缩放、`F12` 开发者工具。
- **外链转系统浏览器**：窗口内的站外链接交给桌面默认程序打开，不接管应用窗口。
- **服务独立于外壳**：服务进程在自身进程组中运行，关闭窗口不会停止它。

## 获取

### 使用 Release 产物

每个 release 附带 Windows 安装包、macOS dmg、deb、rpm 与 AppImage，并附 `SHA256SUMS`。
deb 仅依赖 `libwebkit2gtk-4.1-0` 与 `libgtk-3-0`：

```sh
sudo apt install "./dsh-xswt-tauriapp_0.0.7_amd64.deb"
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
| `DSH_SHELL_WAYLAND` | 在 Linux 上不要切到 X11 后端（见「菜单、托盘与快捷键」） |

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

服务日志是 token 的唯一来源，因此在终端手动启动、日志未落在该路径的实例不会被识别，外壳会另起一个实例。这是 dsh 的鉴权模型决定的，外壳不额外猜测。

### 端口选择

| 情况 | 行为 |
|---|---|
| 已发现可复用的服务 | 端口框预填它的端口，确认即复用；填别的端口会在新端口再起一个实例 |
| 没有可复用的服务 | 默认 = `3080`–`3129` 中第一个空闲端口；手输过的端口若仍空闲则优先 |
| 端口被别的程序占用 | 当场提示，确认不会继续 |
| 端口上是 dsh，但本机无法接手其会话 | 单独说明 —— 例如在 Windows 侧启动的实例，或用了另一个 `DSH_HOME` |
| 低于 `1024` | 直接拒绝，普通用户无法绑定 |

只有**手输**过的端口会被记住并作为下次的默认值；接受灰色默认值不算选择，默认值因此继续跟随「第一个空闲端口」。

发现候选端口来自 `$DSH_HOME/launcher/logs/server-<port>.out.log` 的**文件名**，而不是扫描整个端口段：一个没有日志的端口没有启动 token，握手不可能完成，探测它没有意义。这也让刻意选用的非标准端口（例如 `9000`）在下次启动时仍能被找到 —— 只扫 `3080`–`3129` 是找不到的。

### 窗口模型与首次导航

| 窗口 | 内容 | 权限 |
|---|---|---|
| `bootstrap` | 外壳自带页面：进度、更新弹窗、失败信息 | 本地源，唯一被授予 IPC capability 的窗口 |
| `dsh` | dsh 的原始界面 | 远端源，不获得外壳 IPC |

dsh 的会话 cookie 带 `SameSite=Strict`。由页面发起的跨站导航不会带上它 —— 这正是「把外壳页面直接导航到 dsh」会停在 dsh 的 401 文本上的原因。外壳保留 dsh 的安全语义，改的是交接顺序：Rust 先完成握手并把 cookie 写入 cookie 存储，**之后**才以 `http://127.0.0.1:<port>/` 创建 dsh 窗口。该窗口的首次导航由宿主发起、没有发起者页面，因此不是跨站请求，Strict cookie 正常发送。

### 菜单、托盘与快捷键

| 平台 | 形态 |
|---|---|
| macOS | 原生系统菜单（含「编辑」菜单，文本框快捷键依赖它） |
| Windows / Linux | 托盘菜单；不挂永久菜单栏，避免占用 dsh 的高度 |

Tauri 的快捷键只能挂在菜单 accelerator 上，而 Windows / Linux 上挂在窗口的菜单就是可见菜单栏。因此这两个平台的快捷键以全局快捷键实现，并且**只在自身窗口获得焦点期间注册**，失焦即注销，不会长期占用整台机器上的 `Ctrl+R`。桌面环境若拒绝发放全局快捷键，外壳照常启动，只是失去快捷键。

Linux 上还有一个前提：`global-hotkey` 通过 X11 抓键，而 **Wayland 原生窗口的按键根本不经过 X 服务器** —— 快捷键会注册成功但永远不触发（这是 Wayland 的设计，不是缺陷）。因此外壳在有 `DISPLAY` 时默认切到 X11 后端（XWayland 在所有 Wayland 桌面上都在），代价是渲染少一点原生感，换来重新载入、缩放与开发者工具可用。设置 `DSH_SHELL_WAYLAND=1` 可退出该行为，显式设置 `GDK_BACKEND` 时以外壳不干预为准。

缩放由 Rust 调用原生 `set_zoom` 完成：WebView 自带的缩放热键在 macOS / Linux 上是靠往页面注入 polyfill 实现的，与「不注入」相冲突。

### 更新检查

通道由版本字符串判定，而非 npm dist-tag —— 后者自身可能指向一个 RC。

| 通道 | 判定 |
|---|---|
| 正式版 | 无预发布后缀，如 `0.1.5` |
| RC | 预发布段以 `rc` 开头，如 `0.1.5-rc.2` |
| Alpha | 预发布段以 `alpha` 开头，如 `0.1.6-alpha.1` |

启动弹窗的候选版本只在稳定度不低于已装版本的通道中选取：已装 RC 时只会被提示 RC 或正式版，Alpha 需在弹窗中主动选择。「不再提示此版本」把该版本写入应用配置目录，且仅抑制该版本。

## 平台与兼容性

| 项 | 状态 |
|---|---|
| Linux（deb / rpm / AppImage） | 支持，由 CI 产出；会话交接与首次导航已实测 |
| Windows（NSIS 安装包） | 构建已接入；核心逻辑经 CI 在 `windows-latest` 上验证，GUI 未实测 |
| macOS（dmg） | 构建已接入，未签名；GUI 未实测 |
| WSLg | 可运行；WebKitGTK 的 GPU 直通不稳，需设 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 与 `WEBKIT_DISABLE_DMABUF_RENDERER=1`。WSLg **没有状态栏**，托盘图标无处显示（图标本身创建成功）；快捷键可用，前提是走 X11 后端 |

首次导航发送 `SameSite=Strict` cookie 这一行为已在 Linux / WebKitGTK 上实测确认；Windows 与 macOS 依赖各自 WebView 对「无发起者导航」的同站判定，尚未实测。

## 开发与验证

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` 不依赖 GUI 工具链，可在没有 `libwebkit2gtk` 的环境中独立构建与测试。`examples/launch.rs` 以与外壳相同的代码路径启动真实服务，并输出准备好的会话（`url=` 与 `cookie=`）；compat CI 用它验证握手并把 token 不进入 URL 这件事钉住。

## License

[MIT](./LICENSE)
