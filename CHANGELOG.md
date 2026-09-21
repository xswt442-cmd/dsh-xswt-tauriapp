# 更新日志

Release Notes 由对应版本段生成；最新版本在前。
英文版见 [CHANGELOG.en.md](CHANGELOG.en.md)。

## Unreleased

### 新增

- 记住缩放因子：`Ctrl/Cmd+=` / `-` / `0` 的结果存入应用配置目录，重启后仍然生效；`DSH_SHELL_ZOOM` 可为一次会话指定初值，环境变量优先于记忆值。

### 修复

- 更新检查与「更新并重启」改为异步命令：registry 请求和 `npm install -g` 不再跑在 webview 的 IPC 回调里，窗口不再在此期间无响应。
- 市场条目把宿主平台与架构改为可注入参数：「无桌面环境则不下载」这条分支此前只在 Linux 上被测到，`node --test` 在 Windows 与 macOS 上会失败。
- 选择启动端口改为实际 bind 一次，不再用 connect 探测：上一次实例的 listener 尚未释放时 connect 会误报空闲，子进程随后以 `EADDRINUSE` 退出。

- 端口候选只取 90 天内的日志：日志只增不减，此前机器用过的每个端口都会在每次启动时被探测一遍。
- 手输端口改由页面显式标记（此前手输的端口恰好等于被建议的默认值就不会被记住）；待安装版本按 semver 校验后才进 npm 参数；缩放因子传递到新建的 dsh 窗口，不再在交接时丢掉。
- 市场条目把「是否已安装」也交给 fixture 决定：Linux / macOS 的候选路径此前是 `/usr/bin`、`/Applications` 这类绝对路径，于是在装了外壳的机器上，「未安装则下载」的测试会开始失败（本机实际 6 个）。
- 市场引导识别 WSL：`xdg-open` 在那里装不了 `.deb`，于是明确说明这一点，并提示 Windows 桌面应改取 `*-setup.exe`。
- 交接安装包失败不再无声：打开器以非零码退出时记一行日志（`xdg-open` 在没有 `.deb` 处理器时就是这样退出的）。
- Windows 上更新 dsh 不再以 `os error 193`（"不是有效的 Win32 应用程序"）失败：npm 在同一个目录里装了一个无扩展名的 POSIX 脚本与 `npm.cmd`、`npm.ps1`，而按目录顺序选中的正是不能启动的那个；现在按平台挑选（Windows 优先 `npm.cmd`），`.cmd` 显式经 `cmd.exe /d /s /c` 运行、逐 token 加引号，并且不再闪出控制台窗口。
- 从启动器推导 npm prefix 改为认两种目录形状：Windows 的 `<prefix>\node_modules` 没有 `lib` 这一层，此前一律推导失败，「用拥有 dsh 的那个 npm 更新」因此只在 Unix 上成立；同时要求候选目录真的持有 npm，`$DSH_HOME/profiles/node_modules` 这类模块根不会被误认成 prefix。
- 服务日志里的启动 token 不再因尾部窗口起点落在多字节字符中间而读不到：256 KiB 尾部按字节读入后宽松解码，此前严格 UTF-8 校验会让整段读取失败、端口被当作没有 token，进而另起一个实例。
- 交接状态在建窗**之前**就置为「正在交接」：此前这一标记落在建窗之后，若首屏加载比它更快，dsh 窗口会一直停在隐藏状态，并在 90 秒后报一个并不成立的加载超时。

### 安全

- 外壳窗口的导航策略收紧为只放行打包进来的资源：它是唯一被授予 IPC 能力的窗口，此前任意 loopback 端口上的页面都能在其中加载并拿到命令面。
- 打开站外链接不再经 `cmd /C start`（Windows）：cmd 会重新解析交来的字符串，于是 URL 查询串里的 `&` 会结束命令并执行其后的内容（dsh 页面里的任何链接都能触达），而 `%VAR%` 即使在引号内也会被展开；改用 `explorer`，它把 URL 当作自己的参数，既不解析也不展开。
- dsh 窗口的导航策略收紧为「只放行交给它的那一份会话」：此前任何 loopback 地址都留在窗口内，但 dsh 的会话 cookie 按签发时的完整 authority 命名，另一个端口、或把同一端口写成 `localhost`，都进不去而只会停在 401 页；这类链接现在交给系统浏览器打开。

### 维护

- Linux 上把安装包交给系统打开之后补一句 `sudo apt install` 提示：`.deb` 通常被归档管理器接管，而不是安装器。
- `docs:check` 现在同时校验五处版本号一致，不再只等打 tag 时由 release 工作流发现；README 的环境变量表补上 `DSH_SHELL_ZOOM`。

## 0.0.9 - 2026-09-19

### 新增

- 市场条目声明详情页截图：新增 `plugins/dsh-desktop-app/screenshots.json` 与 `assets/screenshot-1-launcher.png`。

### 维护

- 发布说明改从默认分支的 `CHANGELOG.md` 生成，不再取自 tag：改过的旧条目不会再被重跑还原。
- 两份 changelog 的条目改为一行一条、只写改了什么；README 把「平台与兼容性」移到「获取」之前。

## 0.0.8 - 2026-09-17

### 新增

- 启动时可选端口：默认值为运行中 dsh 的端口，否则是首个空闲端口（`3080`–`3129`）或上次手输的端口；也可输入任意端口。
- 端口可用性在启动前判定，区分将启动 / 可复用 / 被占用 / 会话无法接手 / 低于 `1024`。
- 手输的端口会被记住，仍空闲时优先于扫描结果。
- 外壳自身的更新检查：新版本在弹窗顶部提示「下载并安装」，按平台挑选本仓库 release 的安装包，比对发布页 `SHA256SUMS` 通过后才交给系统安装器；本平台没有安装包时退化为打开发布页，校验不符或缺校验文件则拒绝且不落盘。
- 插件市场入口 `plugins/dsh-desktop-app/`：零依赖的引导插件（声明 `dsh.bundle`），首次启动 dsh 时按平台取 `SHA256SUMS`、校验通过才写盘再交给系统安装器；已安装时保持安静，无桌面会话或本平台无安装包时只打印一句说明，不静默安装，也不导入 harness API。

### 变更

- 启动弹窗改为每次启动都显示（标题「启动 dsh」，主按钮「打开 dsh」）；更新检查与端口扫描并行。
- 候选端口改为从 `$DSH_HOME/launcher/logs` 的日志文件名派生，不再扫描 `3080`–`3129`；扫描使用独立的短连接超时，非标准端口下次仍能被发现。

### 修复

- 菜单或托盘构建失败不再阻止应用启动（此前在没有状态栏宿主的桌面上会打不开）。
- 弹窗的「已安装位置」改为实际的 dsh 启动器路径（此前是回环 URL）。
- 版本比较先剥离 tag 的 `v` 前缀，并要求两边都可解析为 semver，不再把当前版本当新版提示。

### 维护

- 发布工作流新增 `stub` 任务：打包市场引导包并以不带版本号的名字 `dsh-xswt-tauriapp-plugin.tgz` 附到 release，断言包内含 `dsh.bundle` 与 `cordis.patch.yml`；版本一致性检查扩到五处（新增 `plugins/dsh-desktop-app/package.json`）。
- 两份 changelog 各自重复的 `## Unreleased` 段已合并为一段（`scripts/release-notes.mjs` 只取第一个匹配段）。

## 0.0.7 - 2026-09-17

### 修复

- 应用与安装包改用项目自己的图标：由 `tauri icon` 生成（`icon.ico` 含 16–256px），并指定 NSIS 的安装器与卸载器图标。
- 发布工作流升级到各 action 的 Node 24 版本，消除 Node 20 弃用告警。

## 0.0.6 - 2026-09-16

### 修复

- Windows 上不再停在 `bootstrap` 页面、无法进入 dsh 界面：建窗命令改为 `async`，窗口仍由主线程创建。
- 快捷键逐把注销并记录，注册失败不再导致其后的快捷键被永久占用。
- 交接失败的原因经 `page_diag` 写进外壳日志（debug 构建，或 release 构建设置 `DSH_SHELL_DEBUG` 时输出到 stderr），不再只渲染在失败页上。
- 本地 `npm run build` 改为跟随 `tauri.conf.json` 的 `targets`，不再写死 Linux 的打包目标。

## 0.0.5 - 2026-09-16

### 变更

- 外壳定位改为 dsh 的 desktop harness：Tauri 负责窗口、进程、启动与复用、会话交接、菜单/托盘/快捷键、更新、外链与故障恢复，页面归 dsh。
- 启动拆为两个窗口：`bootstrap` 承载进度、更新弹窗与失败信息，`dsh` 窗口只显示 dsh；启动期界面不可能覆盖在 dsh 上，这是结构保证而非约定。
- 会话交接移到 Rust：`resolve_session()` 完成并校验握手后返回干净地址与会话 cookie，启动 token 不进入页面 URL。
- 首次导航改为宿主发起，dsh 的 `SameSite=Strict` 保持不变。
- 菜单、托盘与快捷键：macOS 用原生系统菜单（含「编辑」菜单），Windows / Linux 用托盘菜单，快捷键仅在自身窗口获得焦点期间注册。
- 缩放改由 Rust 调用原生 `set_zoom`；开发者工具由 `devtools` feature 提供，release 构建默认不出现在菜单中。

### 移除

- 注入 dsh 页面的 `initialization_script`、401 文本嗅探、交接失败哨兵路径与页面内的 `Ctrl+R` 监听都已移除；外壳不再依赖 guest 的前端结构。

### 修复

- 交接不再停在 dsh 的 401 页面：会话 cookie 在建窗之前写入 cookie 存储并确认可读，并补上服务未给出的 `Domain`。
- `dsh` 窗口在页面加载完成前保持隐藏；加载超时回到 `bootstrap` 页面并说明原因。
- compat CI 断言示例输出的是不含 token 的干净地址，且该地址不带 cookie 时仍返回 401。
- Windows 构建不再因 `global-hotkey` 的 manager 而失败：manager 改放创建它的线程（`thread_local`），managed state 只保留纯数据。
- Linux 上不再出现「快捷键注册成功却不触发」：有 `DISPLAY` 时改用 X11 后端（XWayland），可用 `DSH_SHELL_WAYLAND=1` 退出。
- compat CI 增加 Windows 与 macOS 的 harness 编译检查；快捷键触发时记录日志。

## 0.0.4 - 2026-09-16

### 新增

- `Ctrl/Cmd+R` 刷新（窗口没有浏览器控件；内容字号与主题启动值需刷新才生效）。

### 修复

- 注入脚本不再在远端 dsh 页面上调用外壳 IPC；诊断只从外壳自身页面上报。

## 0.0.3 - 2026-09-16

### 修复

- 交接失败后的重试改在 dsh origin 上进行，`SameSite=Strict` 的会话 cookie 因此能随请求发送，不再停在 401 文本上。

## 0.0.2 - 2026-09-16

### 修复

- 从桌面或文件管理器启动时，`node` / `npm` 改为按 dsh 自身的安装位置推导，不再取 `PATH` 上的第一个。
- 落到 dsh 鉴权页时不再黑屏：重新解析地址重试一次，仍失败则回到外壳页面并给出原因与下一步。

## 0.0.1 - 2026-09-15

### 新增

- 服务复用与启动：扫描 `3080`–`3129`，复用已在运行的 `dsh web`，否则在首个空闲端口以独立进程组启动。
- 启动 token 两步握手：从服务日志读取 token，`GET /?token=…` 换取会话 cookie 后复请并校验页面标记；握手不成立时，端口上的其他服务不会被误认为 dsh。
- 启动时更新检查：读取 npm 上 `@deepseek-ai/dsh` 的已发布版本，按正式版 / RC / Alpha 分栏展示，可安装后重启。
- 更新候选只取自稳定度不低于已装版本的通道。
- 「不再提示此版本」按版本记录于应用配置目录，仅抑制该版本。
- 站外链接交由系统默认程序打开。
- 打包 Windows 安装包、macOS dmg、deb、rpm 与 AppImage；release 关闭调试信息并启用 LTO 与体积优化。

### 安全

- Tauri capability 仅覆盖外壳自带的本地页面；交接后加载的 dsh 界面属远端源，不获得外壳 IPC 权限。
- 更新安装调用与已解析 `node` 同目录的 `npm`，不依赖 `PATH`。

## License

[MIT](./LICENSE)
