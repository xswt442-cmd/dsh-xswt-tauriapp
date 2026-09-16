# 更新日志

Release notes 由对应版本段生成；最新版本在前。
英文版见 [CHANGELOG.en.md](CHANGELOG.en.md)。

## 0.0.5 - 2026-09-16

### 变更

- 外壳重做为 dsh 的 desktop harness：Tauri 只负责窗口、进程、启动与复用、会话交接、菜单/托盘/快捷键、更新、外链与故障恢复，页面内容完全交给 dsh。
- 启动拆成两个窗口：`bootstrap` 承载进度、更新弹窗与失败信息，`dsh` 窗口只显示 dsh 界面。启动过程中的任何界面都不可能覆盖在 dsh 上，这是结构上的保证而非约定。
- 会话交接移到 Rust 侧：新增 `resolve_session()`，走完并校验握手后返回干净地址与会话 cookie，窗口拿到的是已经准备好的会话，启动 token 不再进入页面 URL 或 `location.search`。
- 首次导航改为由宿主发起。dsh 的 `SameSite=Strict` 保持不变，跨站导航扣留 cookie 的问题改由交接顺序解决，而不是放宽 dsh 的安全语义。
- 新增菜单、托盘与快捷键：macOS 用原生系统菜单（含文本框快捷键所依赖的「编辑」菜单），Windows / Linux 用托盘菜单，快捷键只在自身窗口获得焦点期间注册。
- 缩放改为 Rust 侧调用原生 `set_zoom`；开发者工具由 `devtools` feature 提供，release 构建默认不出现在菜单中。

### 移除

- 移除注入 dsh 页面的 `initialization_script`、401 文本嗅探、交接失败哨兵路径，以及页面里的 `Ctrl+R` 监听。外壳不再依赖 guest 的任何前端结构。

### 修复

- 交接不再停在 dsh 的 401 页面：会话 cookie 在 dsh 窗口创建之前写入 cookie 存储并确认可读，同时补上服务未给出的 `Domain`。
- `dsh` 窗口在页面加载完成之前保持隐藏；加载超时会回到 `bootstrap` 页面说明原因，而不是留下一个空窗口。
- compat CI 现在断言示例输出的是干净地址、token 不在其中，并确认同一地址不带 cookie 时仍返回 401。
- Windows 构建不再因 `global-hotkey` 的 manager 而失败。它在 Windows 上是一个裸 `HWND`，既不是 `Send` 也不是 `Sync`，因此不能作为 Tauri managed state（套 `Mutex` 也无效，`Mutex<T>: Sync` 需要 `T: Send`）。manager 现在放在创建它的线程上（`thread_local`），managed state 只保留纯数据。
- Linux 上快捷键不再可能「注册成功却永不触发」。`global-hotkey` 通过 X11 抓键，而 Wayland 原生窗口的按键不经过 X 服务器 —— 这是 Wayland 的设计。外壳现在在有 `DISPLAY` 时使用 X11 后端（XWayland），可用 `DSH_SHELL_WAYLAND=1` 退出。
- compat CI 增加 Windows 与 macOS 上的 harness 编译检查。此前 harness 只在 Linux 上编译过，这类平台相关的类型差异要到打包 release 时才暴露。快捷键触发时也会记一条日志，"注册了但不触发"与"没按"从此可区分。

## 0.0.4 - 2026-09-16

### 新增

- 补上 `Ctrl+R` / `F5` 刷新 —— 窗口没有浏览器控件。dsh 有一部分设置（内容字号、主题的启动取值）由宿主渲染 index 时写入，页面加载之后没有任何代码再改它们，因此这类设置必须刷新才生效；此前外壳不提供任何刷新入口。

### 修复

- 注入脚本不再在远端页面（dsh 界面）上尝试调用外壳 IPC。那里本就被拒绝，且返回的 promise 若被拒绝会再次触发同一个处理器，形成循环。诊断现在只在 shell 自身页面上报，远端页面只保留交接失败这一个必要信号。

## 0.0.3 - 2026-09-16

### 修复

- 交接失败不再留下黑屏。dsh 的会话 cookie 带 `SameSite=Strict`，而外壳页面到 dsh 是一次跨站导航，于是 303 之后那次请求不会带上该 cookie，界面就停在 dsh 的 401 文本上。注入脚本现在会识别这一页并导航到一个哨兵路径，外壳拦截后重新解析地址再试一次 —— 此时浏览器已在 dsh origin 上，属同站导航，cookie 正常发送。

## 0.0.2 - 2026-09-16

### 修复

- 从桌面或文件管理器启动时，`node` 与 `npm` 改为按 dsh 自身的安装位置推导，不再取 `PATH` 上的第一个。那里的第一个通常是更旧的系统 node（本机为 v18），用它启动的 dsh 服务根本起不来；其 `npm install -g` 又指向 `/usr/local`（普通用户不可写），于是「更新并重启」既不更新、也不说明原因。
- 交接落到 dsh 的鉴权页时不再停在黑屏：会重新解析地址重试一次，仍失败则回到外壳页面，显示原因与可执行的下一步。

## 0.0.1 - 2026-09-15

### 新增

- 服务复用与启动：扫描 3080–3129 端口段，复用已在运行的 `dsh web`，否则在首个空闲端口以独立进程组启动一个。
- 启动 token 两步握手：从服务日志读取 token，`GET /?token=…` 取会话 cookie 后复请并校验页面标记；无法握手时不把端口上的其他服务误认为 dsh。
- 启动时更新检查：读取 npm 上 `@deepseek-ai/dsh` 的已发布版本，按正式版 / RC / Alpha 三栏展示并可安装后重启。
- 更新候选仅取自稳定度不低于已装版本的通道，避免自动向 RC 或正式版用户推荐 Alpha。
- 「不再提示此版本」按版本记录于应用配置目录，仅抑制该版本。
- 窗口内站外链接交由系统默认程序打开。
- 打包 Windows 安装包、macOS dmg、deb、rpm 与 AppImage；release 关闭调试信息并启用 LTO 与体积优化。

### 安全

- Tauri capability 仅覆盖外壳自带的本地页面。窗口交接后加载的 dsh 界面属远端源，不获得外壳 IPC 权限。
- 更新安装调用与已解析 `node` 同目录的 `npm`，不依赖 `PATH` 上可能指向其他前缀的 `npm`。

## License

[MIT](./LICENSE)
