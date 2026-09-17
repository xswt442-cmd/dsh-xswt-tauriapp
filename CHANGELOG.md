# 更新日志

Release Notes 由对应版本段生成；最新版本在前。
英文版见 [CHANGELOG.en.md](CHANGELOG.en.md)。

## Unreleased

### 新增

- 启动时可以选择端口。弹窗在版本三栏下方给出端口框，灰色数字是默认值（`3080`–`3129` 中第一个空闲端口，手输过的端口优先）；确认即按默认值启动，也可以输入任意端口。
- 端口可用性当场判定，并区分可复用 / 被别的程序占用 / 是 dsh 但本机无法接手其会话（在 Windows 侧启动，或用了另一个 `DSH_HOME`）/ 低于 `1024` 四种情况，不再等到启动失败。
- 手输过的端口会被记住，作为下次的默认值。
- 外壳自身的更新检查。启动时并行查询本仓库的 GitHub Releases，有新版本就在弹窗顶部提示并给出「下载并安装」：按平台挑选安装包（Windows `setup.exe`、macOS `dmg`、Linux `deb`），比对发布页的 `SHA256SUMS` 通过后才交给系统安装器；发布里没有本平台安装包时退化为「打开发布页」，校验不通过或缺少校验文件则拒绝下载并**不落盘**。
- 插件市场入口 `plugins/dsh-desktop-app/`。零依赖的引导插件，声明市场要求的 `dsh.bundle`：**首次**启动 dsh 时按平台从最新 release 挑选安装包，先取 `SHA256SUMS`、校验通过才写盘，再交给系统安装器打开。已经装好外壳、没有桌面会话（`CI`，或 Linux 上没有 `DISPLAY`/`WAYLAND_DISPLAY`）、本平台没有安装包（`linux/arm64`、`win32/arm64`）三种情况只打印一句说明。它不静默安装，也不导入任何 harness API。

### 变更

- 弹窗改为每次启动都出现（标题「启动 dsh」，主按钮「打开 dsh」）。此前只在有更新时出现，没有可以选端口的时机；更新检查与端口扫描并行，确认不等待 registry。
- 候选端口的发现改从 `$DSH_HOME/launcher/logs` 的日志文件名派生，不再扫描整个 `3080`–`3129`，扫描另用更短的连接超时。这既让非标准端口下次仍能被发现，也消除了 Windows 上无服务时约 40 秒的冷启动。

### 修复

- 菜单或托盘构建失败不再阻止应用启动。两者都只是便利设施，此前 `setup` 会把它们的失败当成启动失败 —— 在没有状态栏宿主的桌面上会打不开，而 macOS 菜单这条路径从未运行过。
- 对话框的「已安装位置」改为实际的 dsh 启动器路径，此前填的是回环 URL。
- 版本比较不再可能把当前版本当成新版本提示。GitHub 的 tag 带 `v` 前缀（`v0.0.7`），而 dsh 那套比较函数在解析失败时会退化成字符串比较，于是 `"v0.0.7" != "0.0.7"` 被判定为有新版本；自更新这条改为先剥离前缀，并要求两边都能解析成 semver。

### 维护

- 发布工作流新增 `stub` 任务：`npm pack` 出市场引导包，以**不带版本号**的名字 `dsh-xswt-tauriapp-plugin.tgz` 附到 release 上，并断言包内确实带着 `dsh.bundle` 与它指向的 `cordis.patch.yml`。名字必须不带版本号，因为 `releases/latest/download/<name>` 只在请求时解析 `latest`，文件名照字面取。版本一致性检查从四个字段扩到五个（新增 `plugins/dsh-desktop-app/package.json`）。
- 两个 changelog 此前各有两个 `## Unreleased` 段，而 `scripts/release-notes.mjs` 只取第一个匹配段，0.0.8 的发布说明会因此丢掉「外壳自身的更新」整段。两段已合并为一段。

## 0.0.7 - 2026-09-17

### 修复

- 应用与安装包改用项目自己的图标。此前 `src-tauri/icons/` 还是 Tauri 脚手架的蓝色圆角方块——仓库、exe、已安装副本三者逐字节相同；Windows 上还有第二处问题：`bundle.windows.nsis` 没有指定图标，安装器脚本里的 `INSTALLERICON` 因此是空串，`setup.exe` 与 `uninstall.exe` 都回落到 NSIS 自带图标。图标现由 `tauri icon` 从品牌图生成（`icon.ico` 含 16/24/32/48/64/256 帧），两处 NSIS 图标设置指向它。
- 发布工作流改用各自默认 Node 24 的 action 版本（`upload-artifact@v6`、`download-artifact@v7`、`pnpm/action-setup@v5`），不再产生 Node 20 弃用告警。

## 0.0.6 - 2026-09-16

### 修复

- Windows 上不再停在 `bootstrap` 页面。创建 `dsh` 窗口的 `open_dsh` 是同步命令，因此运行在 webview 自己的 IPC 回调里，而 Windows 上从该回调中再建一个 webview 会死锁（wry#583）：窗口已经建出来却永远不返回，`dsh` 窗口就一直保持隐藏。它现在是 `async` 命令，窗口仍由主线程创建。0.0.5 的 Windows 安装包可以安装并启动，但界面到不了 dsh。
- 快捷键不再可能被永久占用。注册失败是正常情况 —— 裸 `F12` 常被其他程序占用 —— 而释放走的是 `unregister_all`，它遇到第一把注销失败的键就停止，排在它之后的键会一直留在被占用状态。现在逐把注销、逐把记录，日志里的持有数量也改为实际注册成功的数量。
- 交接失败的原因现在会写进外壳日志。`open_dsh` 被拒绝时只渲染失败页，而打包后的 GUI 没有终端，这条最关键的失败因此不留痕迹；它现在和其他失败一样经 `page_diag` 上报到 stderr。
- 本地 `npm run build` 不再写死 Linux 的打包目标（`deb,rpm,appimage`），改为跟随 `tauri.conf.json` 的 `targets`，因此在 Windows 与 macOS 上同样可用。

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
