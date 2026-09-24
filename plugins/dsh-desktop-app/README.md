# dsh-desktop-app

Marketplace stub for **[dsh-xswt-tauriapp](https://github.com/xswt442-cmd/dsh-xswt-tauriapp)** — a
lightweight Tauri desktop shell for DeepSeek Harness that reuses or starts a local `dsh web` server
and embeds it in a native window.

The shell itself is a native application published as a GitHub release artifact, so there is no npm
package to install. This package is what the plugin market can install: it declares the `dsh.bundle`
manifest, and on the **first `dsh` start after installation** it

1. checks whether the shell is already installed (and says nothing if it is),
2. reads the shell's latest GitHub release and picks this platform's installer from its asset list,
3. downloads `SHA256SUMS`, downloads the installer, and verifies the digest **before writing anything**
   to disk (`$DSH_HOME/dsh-xswt-tauriapp/updates/`, where it also removes the installers it supersedes),
4. hands the verified file to the operating system — `start` on Windows, `open` on macOS, `xdg-open`
   on Linux — and records that it did so in `$DSH_HOME/dsh-xswt-tauriapp/plugin.json`.

Every later start is silent. On WSL the Linux asset is a `.deb`, which `xdg-open` cannot install: the
plugin says so, names the `sudo apt install <path>` that does, and points at the Windows installer if
the desktop is the Windows one. A hand-off that fails is a log line, never a no-op.

## What it deliberately does not do

It does not install anything, and it does not run an installer silently. An `.exe` has to pass
SmartScreen, a `.dmg` has to pass Gatekeeper, and a `.deb` needs the distribution's own dependency
resolution and root — a plugin that fights any of those fails in a way the user never sees. The
installer is opened, and the user finishes it.

Nor does it import a single harness API. It imports Node built-ins only: no `@deepseek-ai/*`, no
tool, no client row, no UI, no dsh internal. A harness change cannot break it, and it cannot become
the thing that stops a harness from booting — the whole plugin is one boot-time check and one
download.

## Install

From the plugin market, or directly:

```sh
dsh plugin --profile web add https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
dsh web
```

Or grab the installer yourself from the
[releases page](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest) — which is all this
stub would have done.

## Configuration

`mode` and `open` live in this bundle's patch row (`cordis.patch.yml`); the environment wins, so a
machine that must never download anything can opt out without editing a profile that `dsh plugin`
rewrites:

| Setting | Environment | Default | Meaning |
|---|---|---|---|
| `mode: auto` | `DSH_TAURIAPP_MODE=auto` | ✓ | download, verify, hand over |
| `mode: notice` | `DSH_TAURIAPP_MODE=notice` | | print the release page, download nothing |
| `mode: off` | `DSH_TAURIAPP_MODE=off` | | say nothing |
| `open: true` | `DSH_TAURIAPP_NO_OPEN=1` disables | ✓ | open the verified installer |

Facts rather than settings: a machine with no desktop session (`CI`, or Linux without `DISPLAY` /
`WAYLAND_DISPLAY`) is only told where the download is, and a release with no installer for this
platform/architecture (`linux/arm64`, `win32/arm64`) is only pointed at the release page.

Two more variables exist for the tests and for a mirror: `DSH_TAURIAPP_RELEASES_API` replaces the
GitHub releases API URL, `DSH_TAURIAPP_FORCE=1` offers the installer again after it has already been
downloaded once.

## Tests

```sh
node --test plugins/dsh-desktop-app/test/plugin.test.js
```

Every case runs against a stand-in release server on loopback: nothing in the suite reaches GitHub,
and `DSH_TAURIAPP_NO_OPEN=1` keeps each run at the point where it would have handed the file over.

## Uninstall

```sh
dsh plugin --profile web remove dsh-xswt-tauriapp-plugin
```

That removes the stub only. The desktop application is uninstalled by the platform's own uninstaller
(Windows: *Apps & features*; macOS: drag the app out of *Applications*; Debian/Ubuntu:
`sudo apt remove dsh-xswt-tauriapp`).

## License

MIT — see [LICENSE](LICENSE).
