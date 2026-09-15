# Changelog

Release notes are generated from the matching version section; newest first.
For Chinese, see [CHANGELOG.md](CHANGELOG.md).

## 0.0.1 - 2026-09-15

### Added

- Server reuse and startup: scans ports 3080–3129, reuses a running `dsh web`, and otherwise starts one on the first free port in its own process group.
- Two-step launch-token handshake: reads the token from the server log, redeems it with `GET /?token=…`, re-requests with the session cookie, and checks the page marker; when the handshake cannot be completed, another service on the port is not mistaken for dsh.
- Update check on launch: reads the published versions of `@deepseek-ai/dsh` from npm and shows them in stable, RC and alpha columns, with the option to install one and restart.
- Update candidates come only from channels at least as stable as the installed version, so an RC or stable install is never prompted towards alpha.
- "Don't remind me about this version" records the version in the app config directory and suppresses only that version.
- Links leaving the app are opened by the desktop's default handler.
- deb and AppImage bundles; the release profile disables debug info and enables LTO and size optimisation.

### Security

- The Tauri capability covers only the shell's own local page. The dsh interface loaded after hand-off is a remote origin and gains no access to the shell's IPC surface.
- Installing an update invokes the `npm` next to the resolved `node`, rather than whichever `npm` happens to be on `PATH` with a different prefix.

## License

[MIT](./LICENSE)
