# 更新日志

Release notes 由对应版本段生成；最新版本在前。
英文版见 [CHANGELOG.en.md](CHANGELOG.en.md)。

## 0.0.1 - 2026-09-15

### 新增

- 服务复用与启动：扫描 3080–3129 端口段，复用已在运行的 `dsh web`，否则在首个空闲端口以独立进程组启动一个。
- 启动 token 两步握手：从服务日志读取 token，`GET /?token=…` 取会话 cookie 后复请并校验页面标记；无法握手时不把端口上的其他服务误认为 dsh。
- 启动时更新检查：读取 npm 上 `@deepseek-ai/dsh` 的已发布版本，按正式版 / RC / Alpha 三栏展示并可安装后重启。
- 更新候选仅取自稳定度不低于已装版本的通道，避免自动向 RC 或正式版用户推荐 Alpha。
- 「不再提示此版本」按版本记录于应用配置目录，仅抑制该版本。
- 窗口内站外链接交由系统默认程序打开。
- 打包 deb 与 AppImage；release 关闭调试信息并启用 LTO 与体积优化。

### 安全

- Tauri capability 仅覆盖外壳自带的本地页面。窗口交接后加载的 dsh 界面属远端源，不获得外壳 IPC 权限。
- 更新安装调用与已解析 `node` 同目录的 `npm`，不依赖 `PATH` 上可能指向其他前缀的 `npm`。

## License

[MIT](./LICENSE)
