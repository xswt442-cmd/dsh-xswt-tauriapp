# dsh-xswt-tauriapp

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
![DSH shell](https://img.shields.io/badge/DSH-shell-4d6bfe)

DeepSeek Harness 的轻量 Tauri 桌面外壳。它复用或启动本机的 `dsh web` 服务、把界面嵌入原生窗口，并在启动时检查 dsh 更新。外壳不修改 dsh 源码，也不介入其会话、沙箱与权限模型。

## 功能

- **服务复用与启动**：扫描 3080–3129 端口段，优先复用已在运行的 `dsh web`，否则在首个空闲端口启动一个。
- **启动 token 握手**：dsh 只把界面交给携带启动 token 的请求。外壳从服务日志读取该 token 并完成两步握手，因此不会把端口上的其他服务误认为 dsh。
- **启动时检查更新**：读取 npm 上 `@deepseek-ai/dsh` 的已发布版本，按正式版 / RC / Alpha 三栏展示，可选定任一版本更新并重启。
- **不再提示此版本**：勾选后该版本不再触发启动弹窗；更新的版本出现时仍会提示。
- **外链转系统浏览器**：窗口内的站外链接交给桌面默认程序打开，不接管应用窗口。
- **保留刷新入口**：窗口没有浏览器控件，`Ctrl+R` / `F5` 重新加载界面。dsh 的部分设置（内容字号、主题启动值）由宿主渲染下标时写入，改完需要刷新才生效。
- **服务独立于外壳**：服务进程在自身进程组中运行，关闭窗口不会停止它。

## 获取

### 使用 Release 产物

每个 release 附带 Windows 安装包、macOS dmg、deb、rpm 与 AppImage，并附 `SHA256SUMS`。
deb 仅依赖 `libwebkit2gtk-4.1-0` 与 `libgtk-3-0`：

```sh
sudo apt install "./dsh-xswt-tauriapp_0.1.0_amd64.deb"
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

外壳先显示自带页面，同时完成服务发现与更新检查，然后交接到 dsh 界面。可用环境变量：

| 变量 | 作用 |
|---|---|
| `DSH_HOME` | DSH 主目录，默认 `~/.dsh` |
| `DSH_BIN` | 直接指定 `dsh` 的 `lib/bin.js` |
| `DSH_NODE_BIN` | 直接指定 `node` 可执行文件 |
| `DSH_TAURI_REGISTRY` | 覆盖版本查询地址，默认 npm registry |
| `DSH_SHELL_DEBUG` | 非空时输出导航与更新检查日志 |

## 工作原理

### 服务发现与握手

| 步骤 | 行为 |
|---|---|
| 端口段 | `3080`–`3129` |
| 复用判定 | 读取 `$DSH_HOME/launcher/logs/server-<port>.out.log` 尾部的启动 token |
| 握手 | `GET /?token=…` → 303 与会话 cookie → 携带 cookie 复请，校验页面含 `DeepSeek Harness` |
| 无认证服务 | 直接 `GET /` 返回 200 且含标记时同样采纳 |
| 启动 | `node <dsh>/lib/bin.js web --port <p> --no-open`，独立进程组，输出追加到上述日志 |

服务日志是 token 的唯一来源，因此在终端手动启动、日志未落在该路径的实例不会被识别，外壳会另起一个实例。这是 dsh 的鉴权模型决定的，外壳不额外猜测。

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
| Linux（deb / rpm / AppImage） | 支持，由 CI 产出 |
| Windows（NSIS 安装包） | 构建已接入；核心逻辑经 CI 在 `windows-latest` 上验证，GUI 未实测 |
| macOS（dmg） | 构建已接入，未签名；GUI 未实测 |
| WSLg | 可运行；WebKitGTK 的 GPU 直通不稳，需设 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 与 `WEBKIT_DISABLE_DMABUF_RENDERER=1` |

## 开发与验证

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` 不依赖 GUI 工具链，可在没有 `libwebkit2gtk` 的环境中独立构建与测试。`examples/launch.rs` 以与外壳相同的代码路径启动真实服务并输出其 URL。

## License

[MIT](./LICENSE)
