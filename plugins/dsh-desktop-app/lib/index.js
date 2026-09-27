/**
 * dsh-xswt-tauriapp — the marketplace stub for the lightweight Tauri desktop
 * shell of DeepSeek Harness.
 *
 * The shell is a native application published as a GitHub release artifact, so
 * there is nothing useful to put on npm: what a storefront needs is something
 * `dsh plugin add` can install and run. This package is that something. It
 * declares the `dsh.bundle` manifest the market requires, and on the first dsh
 * start after installation it downloads this platform's installer, verifies it
 * against the release's `SHA256SUMS`, and hands the verified file to the
 * operating system.
 *
 * It never installs anything itself, and it never runs an installer silently.
 * An `.exe` asks SmartScreen, a `.dmg` asks Gatekeeper, and a `.deb` needs the
 * distribution's own dependency resolution and root — a plugin that fights any
 * of those is a plugin that fails in a way the user cannot see.
 *
 * Deliberately free of harness APIs: no `@deepseek-ai/*` import, no tool, no
 * client row, no UI. It reads no dsh internal, so a harness change cannot break
 * it, and — the reason this shape was chosen over a bundle that ships the app —
 * it cannot be the thing that stops a harness from booting.
 *
 * @module dsh-xswt-tauriapp-plugin
 */

import { createHash } from 'node:crypto'
import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, delimiter, join } from 'node:path'

/** Stable Cordis plugin name. */
const name = 'xswt-tauriapp'

/** This repository's coordinates: the release lookup and the release page. */
const REPO = 'xswt442-cmd/dsh-xswt-tauriapp'
const DEFAULT_RELEASES_API = `https://api.github.com/repos/${REPO}/releases/latest`
const RELEASES_PAGE = `https://github.com/${REPO}/releases/latest`
const USER_AGENT = 'dsh-xswt-tauriapp-plugin'

/** The one asset every release carries beside its installers. */
const CHECKSUMS = 'SHA256SUMS'

/**
 * How long a request may take before it is called a failure, in milliseconds:
 * the stub's mirror of the shell's own `API_TIMEOUT` and `DOWNLOAD_TIMEOUT`
 * (`crates/dsh-core/src/self_update.rs`). A `fetch` with no deadline does not
 * fail, it waits — and this one runs on a harness's boot path, where a stalled
 * connection used to mean a silent hang and no log line at all.
 */
const API_TIMEOUT = 15_000
const DOWNLOAD_TIMEOUT = 300_000

/** The installed application's name, in its bundle and its executable. */
const APP = 'dsh-xswt-tauriapp'

/**
 * Every suffix an installer this application publishes ends in, across every
 * platform it builds for. Pruning recognises its own downloads by these, so the
 * list has to cover everything `installerSuffix` can return — a test holds the
 * two together rather than a comment.
 */
const INSTALLER_SUFFIXES = ['_x64-setup.exe', '_x64.dmg', '_aarch64.dmg', '_amd64.deb']

/**
 * Log prefix. The launcher writes a spawned `dsh web` stdout to
 * `$DSH_HOME/launcher/logs/`, which is where someone looking into a missing
 * download reads it.
 */
const PREFIX = '[dsh-xswt-tauriapp]'

/** The three answers to "what should this plugin do". */
const MODES = ['auto', 'notice', 'off']
const DEFAULT_CONFIG = { mode: 'auto', open: true }

/**
 * Merge the patch row's config with the environment, which wins: a machine that
 * must never download anything is opted out in its environment, not by editing
 * a profile every `dsh plugin` command rewrites.
 * @param config - the row's `config` object, if any.
 * @returns a mode from `MODES` and whether to open what was downloaded.
 */
function resolveConfig(config) {
  const raw = config !== null && typeof config === 'object' ? config : {}
  const fromEnv = process.env.DSH_TAURIAPP_MODE
  const mode = MODES.includes(fromEnv) ? fromEnv : MODES.includes(raw.mode) ? raw.mode : DEFAULT_CONFIG.mode
  return { mode, open: typeof raw.open === 'boolean' ? raw.open : DEFAULT_CONFIG.open }
}

/**
 * The release asset this platform installs from, mirroring the shell's own
 * `installer_suffixes`. A Tauri bundle name carries its version, so an asset is
 * always chosen from the release's asset list — never guessed as a URL — and
 * `latest/download/<versioned name>` is a link that would rot on the next tag.
 * @param platform - `process.platform`.
 * @param arch - `process.arch`.
 * @returns the asset-name suffix, or `undefined` when this platform has none.
 */
function installerSuffix(platform = process.platform, arch = process.arch) {
  if (platform === 'win32' && arch === 'x64') return '_x64-setup.exe'
  if (platform === 'darwin' && arch === 'arm64') return '_aarch64.dmg'
  if (platform === 'darwin' && arch === 'x64') return '_x64.dmg'
  if (platform === 'linux' && arch === 'x64') return '_amd64.deb'
  return undefined
}

/**
 * Whether handing an installer to this machine could help at all. A server, a
 * container and a CI runner have no desktop to open one on, and fetching three
 * megabytes there is noise rather than a favour — those hosts are told where the
 * release page is instead.
 * @param platform - `process.platform`.
 * @param env - the environment to read.
 * @returns whether a desktop session appears to exist.
 */
function hasDesktop(platform = process.platform, env = process.env) {
  if (env.CI !== undefined && env.CI !== '' && env.CI !== 'false') return false
  if (platform !== 'linux') return true
  return Boolean(env.DISPLAY || env.WAYLAND_DISPLAY)
}

/**
 * dsh's home, resolved the way the launcher resolves it.
 * @param env - the environment to read.
 * @returns the absolute `$DSH_HOME`.
 */
function dshHome(env = process.env) {
  return env.DSH_HOME !== undefined && env.DSH_HOME !== '' ? env.DSH_HOME : join(homedir(), '.dsh')
}

/**
 * Where this plugin records what it already offered. It lives under `$DSH_HOME`
 * rather than the system temp directory because it is the answer to "has this
 * been done for this person", and a reboot should not ask again.
 * @param env - the environment to read.
 * @returns the absolute state-file path.
 */
function statePath(env = process.env) {
  return join(dshHome(env), APP, 'plugin.json')
}

/**
 * Whether a release asset's name can be used as a file name at all.
 *
 * The name comes over the network, and `DSH_TAURIAPP_RELEASES_API` is a
 * documented override — a mirror, or somebody else's release JSON, is a real
 * input here. The chosen name becomes a path component under `$DSH_HOME` *and*
 * part of the platform opener's argument list, and it is also the key looked up
 * in `SHA256SUMS`, so a name that climbs out of the updates directory still
 * verifies: the digest cannot be the check. This is.
 *
 * A bare file name means no directory part in either spelling, no traversal, no
 * colon (a drive or an alternate data stream), and no control character. The
 * name never reaches a shell as a command of its own: it is always joined to the
 * absolute updates directory first, so it cannot read as an option either.
 * @param name - a release asset's file name.
 * @returns whether it is safe to write and to hand over.
 */
function isBareFileName(name) {
  if (typeof name !== 'string' || name === '') return false
  if (name.includes('/') || name.includes('\\')) return false
  if (name === '.' || name === '..' || name.includes('..')) return false
  if (name.includes(':')) return false
  // NUL and the rest of the control range. They make file names that cannot be
  // printed and logs that can be split.
  return !/[\u0000-\u001f\u007f]/.test(name)
}

/**
 * Where a verified installer is written: under `$DSH_HOME`, beside the state
 * file, and for the same reason. Both the recorded state and the log line name
 * this path as the `sudo apt install <path>` a person can still run tomorrow,
 * and a temporary directory is emptied by a reboot — which made a download that
 * verified correctly look like a file that had never been written.
 *
 * The name is checked here as well as where it is chosen, because this is the
 * line that turns a name into a path: nothing that reaches it may escape the
 * directory it belongs in.
 * @param installerName - the installer's file name.
 * @param env - the environment to read.
 * @returns the absolute path to write it to.
 */
function installerPath(installerName, env = process.env) {
  if (!isBareFileName(installerName)) {
    throw new Error(`${JSON.stringify(installerName)} is not a bare file name, so it is not written under ${dshHome(env)}`)
  }
  return join(dshHome(env), APP, 'updates', installerName)
}

/**
 * Remove this application's superseded installers from `dir`, keeping `keep`.
 *
 * The directory outlives the session now, so without this it would collect one
 * installer per release. Only files named after this application *and* ending in
 * a platform installer suffix are removed: anything else in there was not put
 * there by a download. A file that cannot be removed is not a reason to fail an
 * install that has already verified.
 * @param dir - the directory the installer was written to.
 * @param keep - the file name to keep.
 * @returns the names removed.
 */
function pruneInstallers(dir, keep) {
  const removed = []
  for (const entry of readdirSync(dir)) {
    const ours = entry.startsWith(APP) && INSTALLER_SUFFIXES.some((suffix) => entry.endsWith(suffix))
    if (entry === keep || !ours) continue
    try {
      rmSync(join(dir, entry))
      removed.push(entry)
    } catch {
      // Left behind rather than made into a failure: the download is verified.
    }
  }
  return removed
}

/** @returns the recorded state, or `{}` when there is none to read. */
function readState(env = process.env) {
  try {
    const state = JSON.parse(readFileSync(statePath(env), 'utf8'))
    return state !== null && typeof state === 'object' ? state : {}
  } catch {
    return {}
  }
}

/** Record what happened, so the next start can stay quiet about it. */
function writeState(state, env = process.env) {
  const path = statePath(env)
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, `${JSON.stringify(state, null, 2)}\n`)
}

/**
 * The directories each platform's installer puts the application in, used when
 * the caller names none. A Linux host that installs under `/opt` keeps a
 * directory of its own, which the executable shape below joins once more.
 */
const INSTALL_DIRS = {
  darwin: ['/Applications'],
  linux: ['/usr/bin', '/usr/local/bin', join('/opt', APP)],
}

/**
 * How a platform's installed copy is spelled inside one of its directories: an
 * `.app` bundle, an installed folder holding the executable, or a bare
 * executable. Decided by the platform, never by who is asking.
 * @param dir - a directory installers write to.
 * @param platform - `process.platform`.
 * @returns the path an installed copy would be at.
 */
function installedIn(dir, platform) {
  if (platform === 'win32') return join(dir, APP, `${APP}.exe`)
  if (platform === 'darwin') return join(dir, `${APP}.app`)
  return join(dir, APP)
}

/**
 * Where the shell would already be if its installer had run. Best effort by
 * design: this only decides whether the plugin stays quiet, and a miss costs one
 * redundant download rather than a wrong action.
 *
 * On macOS and Linux the directories used to be hardcoded absolute paths —
 * `/Applications`, `/usr/bin` — which made this a question about the machine
 * running the tests rather than about the host a test names: install the
 * application to try it, and every test that expects a download starts finding an
 * installed copy instead. The fix was a fork on whether `env` *was* `process.env`
 * (object identity), and that left the tested path and the shipped path as
 * different code: a fixture could only ever express `$HOME/.local/bin`, while
 * every real host probed the absolute directories.
 *
 * The directories are data now. `DSH_TAURIAPP_INSTALL_DIRS` — a path list, so
 * split on `path.delimiter` — replaces them wholesale for a caller describing a
 * host it is not on, exactly as `LOCALAPPDATA` and `ProgramFiles` already did, and
 * what a directory *contains* is decided by `platform`. One code path, so a test
 * can name any host's filesystem, installed or not, and the entry point can be
 * driven on a machine that really has the application.
 * @param platform - `process.platform`.
 * @param env - the environment to read.
 * @returns candidate paths, any of which means "already installed".
 */
function installedCandidates(platform = process.platform, env = process.env) {
  const listed = env.DSH_TAURIAPP_INSTALL_DIRS
  const dirs =
    listed === undefined || listed === ''
      ? platform === 'win32'
        ? [
            env.LOCALAPPDATA,
            env.LOCALAPPDATA !== undefined ? join(env.LOCALAPPDATA, 'Programs') : undefined,
            env.ProgramFiles,
            env['ProgramFiles(x86)'],
          ].filter(Boolean)
        : platform === 'darwin'
          ? INSTALL_DIRS.darwin
          : INSTALL_DIRS.linux
      : listed.split(delimiter).filter(Boolean)
  const candidates = dirs.map((dir) => installedIn(dir, platform))
  // A home bin is where a person installs things themselves, on either unix, and a
  // fixture owns its own `HOME` — so it stays probeable without naming the
  // machine's absolute directories.
  if (platform !== 'win32' && env.HOME !== undefined && env.HOME !== '') {
    candidates.push(join(env.HOME, '.local', 'bin', APP))
  }
  return candidates
}

/** @returns whether the application is already installed. */
function isInstalled(platform = process.platform, env = process.env) {
  return installedCandidates(platform, env).some((candidate) => existsSync(candidate))
}

/**
 * Whether an error is a missed deadline rather than an answer the server sent.
 *
 * `AbortSignal.timeout` rejects with a `TimeoutError`, and Node's `fetch` either
 * surfaces that directly or wraps it, so both shapes count.
 * @param error - whatever was thrown.
 * @returns whether the request ran out of time.
 */
function isTimeout(error) {
  const timedOut = (candidate) => candidate !== null && typeof candidate === 'object' && candidate.name === 'TimeoutError'
  return timedOut(error) || timedOut(error?.cause)
}

/**
 * A request with a deadline that reports itself when it is missed.
 *
 * Without a signal, a connection that stalls mid-handshake leaves `run()` waiting
 * forever on a harness's boot path: not a failure, not a log line, nothing. Every
 * fetch in this module goes through here so a stall becomes an ordinary reported
 * failure, with the timeout's own reason kept as the cause.
 * @param what - how to name the request when it stalls.
 * @param timeout - the deadline, in milliseconds.
 * @param request - the fetch to run, including reading its body.
 * @returns whatever `request` resolves to.
 */
async function withTimeout(what, timeout, request) {
  try {
    return await request()
  } catch (error) {
    if (isTimeout(error)) {
      throw new Error(`${what} did not answer within ${timeout / 1000}s`, { cause: error })
    }
    throw error
  }
}

/**
 * Read the release the shell's own updater reads.
 * @param api - the releases API URL, `DSH_TAURIAPP_RELEASES_API` if set.
 * @param timeout - how long the lookup may take; `API_TIMEOUT` in production.
 * @returns the version, the release page and the asset list.
 */
async function fetchRelease(api, timeout = API_TIMEOUT) {
  const release = await withTimeout('the release lookup', timeout, async () => {
    const response = await fetch(api, {
      headers: { accept: 'application/vnd.github+json', 'user-agent': USER_AGENT },
      signal: AbortSignal.timeout(timeout),
    })
    if (!response.ok) throw new Error(`the release lookup answered HTTP ${response.status}`)
    return response.json()
  })
  const assets = Array.isArray(release.assets) ? release.assets : []
  return {
    version: String(release.tag_name ?? '').replace(/^[vV]/, ''),
    page: typeof release.html_url === 'string' && release.html_url !== '' ? release.html_url : RELEASES_PAGE,
    assets: assets.map((asset) => ({
      name: String(asset.name ?? ''),
      url: String(asset.browser_download_url ?? ''),
    })),
  }
}

/** @returns the first asset whose name the predicate accepts. */
function assetNamed(release, accepts) {
  return release.assets.find((asset) => accepts(asset.name))
}

/**
 * Parse `sha256sum` output — a hex digest, whitespace, then the file name —
 * into a name-to-digest map. The release computes one file for every artifact,
 * so the installer's own line is the one that matters.
 * @param text - the `SHA256SUMS` contents.
 * @returns a map from file name to lowercase digest.
 */
function parseSha256Sums(text) {
  const sums = new Map()
  for (const line of String(text).split(/\r?\n/)) {
    const match = line.match(/^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$/)
    if (match) sums.set(match[2], match[1].toLowerCase())
  }
  return sums
}

/**
 * A download is only written to disk once its digest matches, so nothing this
 * plugin leaves behind is something the release did not vouch for.
 * @returns the digest of what was downloaded and whether it is the expected one.
 */
function verifyDigest(bytes, expected) {
  const actual = createHash('sha256').update(bytes).digest('hex')
  return { actual, ok: actual === expected.toLowerCase() }
}

/** @returns the bytes at a URL, following GitHub's asset redirect. */
async function download(url, timeout = DOWNLOAD_TIMEOUT) {
  return withTimeout('the download', timeout, async () => {
    const response = await fetch(url, {
      headers: { 'user-agent': USER_AGENT },
      signal: AbortSignal.timeout(timeout),
    })
    if (!response.ok) throw new Error(`the download answered HTTP ${response.status}`)
    // The body counts against the same deadline: an asset that stops streaming
    // half-way would otherwise hang here rather than at the request.
    return Buffer.from(await response.arrayBuffer())
  })
}

/**
 * An error in the words that actually diagnose it.
 *
 * Node's `fetch failed` is a `TypeError` that keeps the real reason — DNS, TLS, a
 * proxy, a refused connection — in `.cause`. A handler that prints only
 * `error.message` reports every network failure as the same contentless line, so
 * the cause is followed until it runs out. A cause that is not an `Error` (a
 * `DOMException`, a bare string) is read the way it would print.
 * @param error - whatever was thrown.
 * @param depth - how deep down the cause chain this call is; the chain is finite.
 * @returns the message, with the cause's message after it when there is one.
 */
function describeError(error, depth = 0) {
  if (error instanceof Error) {
    const message = error.message !== '' ? error.message : error.name
    if (error.cause === undefined || error.cause === null || depth >= 3) return message
    const cause = describeError(error.cause, depth + 1)
    return cause === '' ? message : `${message}: ${cause}`
  }
  if (typeof error === 'string' || typeof error === 'number' || typeof error === 'boolean') return String(error)
  if (error === null || error === undefined) return ''
  // A `DOMException` and friends: `String` gives `Name: message`, which is the
  // part worth reading, and never throws the way a getter might.
  return String(error)
}

/**
 * The command a person can type if the automatic handoff does nothing — the
 * usual outcome on a Linux desktop with no handler registered for `.deb`.
 * @param path - the verified installer.
 * @param platform - `process.platform`.
 * @returns a one-line command or instruction.
 */
function manualCommand(path, platform = process.platform) {
  if (platform === 'win32') return `start "" "${path}"`
  if (platform === 'darwin') return `open "${path}"`
  return `sudo apt install "${path}"`
}

/**
 * The opener this platform runs to hand a file over, as data rather than as a
 * spawn. Kept apart from `openInstaller` so the argument shape is pinned by a
 * test on every platform, including the one the test is not running on.
 * @param path - the file to hand over.
 * @param platform - `process.platform`.
 * @returns the program and the arguments it is handed.
 */
function openerCommand(path, platform = process.platform) {
  if (platform === 'win32') return { file: 'cmd.exe', args: ['/c', 'start', '', path] }
  if (platform === 'darwin') return { file: 'open', args: [path] }
  return { file: 'xdg-open', args: [path] }
}

/**
 * Hand a verified file to the platform opener. Spawning a detached child is the
 * whole of it: the user answers SmartScreen, Gatekeeper or the package manager,
 * which is exactly the boundary this plugin promises.
 *
 * The child is not waited for — an installer window outlives us — but its exit is
 * not discarded either. `xdg-open` on a machine with no handler for `.deb` runs,
 * exits non-zero and opens nothing, and a handoff that fails in silence is the one
 * outcome this module exists to avoid. On Windows `start` answers nothing: it exits
 * 0 whether or not it opened the file, so a silent hand-off there is reported by
 * the verified path in the log rather than by the child.
 * @param path - the verified installer.
 * @param platform - `process.platform`.
 * @param log - where a failed handoff is reported.
 * @param spawnImpl - `spawn`, or a test's stand-in for it. This is the only place
 *   in the module that starts a process, so it is also the only place a test can
 *   watch: the seam lets a test record the argv a real child was handed without
 *   ever running an installer.
 */
function openInstaller(path, platform = process.platform, log = console.log, spawnImpl = spawn) {
  const command = openerCommand(path, platform)
  try {
    const child = spawnImpl(command.file, command.args, { detached: true, stdio: 'ignore', windowsHide: true })
    child.on('error', (error) => log(`${PREFIX} could not run ${command.file}: ${error.message}`))
    child.on('exit', (code) => {
      if (code !== 0) {
        log(`${PREFIX} ${command.file} exited ${code} without opening it; finish it yourself with: ${manualCommand(path, platform)}`)
      }
    })
    child.unref()
  } catch (error) {
    log(`${PREFIX} could not run ${command.file}: ${describeError(error)}`)
  }
}

/**
 * Whether this is a WSL distribution.
 *
 * The environment is what WSL sets for anything it starts, and `/proc/version` is
 * the fallback for a process that did not inherit it. The fallback is consulted
 * only when the caller passed the real environment: a fixture describes a host, it
 * does not run on one, so a test stays deterministic wherever it runs.
 * @param env - the environment to read.
 * @returns whether the host is WSL.
 */
function isWsl(env = process.env) {
  if (env.WSL_DISTRO_NAME || env.WSL_INTEROP) return true
  if (env !== process.env) return false
  try {
    return readFileSync('/proc/version', 'utf8').toLowerCase().includes('microsoft')
  } catch {
    return false
  }
}

/**
 * The extra line WSL needs.
 *
 * A `.deb` is the right asset for a Linux host, but inside WSL the desktop the
 * person is looking at is usually the Windows one, and `xdg-open` cannot install a
 * `.deb` even when it does have a handler. Saying so is the difference between a
 * silent no-op and a next step.
 * @param log - where to report.
 * @param platform - the host the installer was chosen for.
 * @param env - the environment to read.
 */
function logWslHint(log, platform, env) {
  if (platform !== 'linux' || !isWsl(env)) return
  log(`${PREFIX} WSL: xdg-open cannot install a .deb — run the apt command above, or take the Windows installer (*-setup.exe) from ${RELEASES_PAGE}`)
}

/**
 * The plugin's whole behaviour: notice the application is missing, fetch this
 * platform's installer, verify it, hand it over, remember that it happened.
 *
 * Every exit is a log line, and the only writes are the installer (once its name
 * proved to be a bare file name and its digest matched) and the state file — both
 * under `$DSH_HOME`, where a reboot cannot take them. Exported through `internals`
 * so the tests can drive it against a stand-in release server.
 *
 * The platform is a parameter like `env` is, not a global: which asset a machine
 * installs from and whether it has a desktop are both answers about a *host*, and
 * a test that cannot name the host can only assert what happens on the machine
 * running it — which is how a headless check passed on Linux and failed
 * everywhere else.
 * @param config - resolved `{ mode, open }`.
 * @param env - the environment to read; the tests pass a fixture.
 * @param log - where to report.
 * @param platform - `process.platform`; the tests name the host they mean.
 * @param arch - `process.arch`.
 * @param handOff - `openInstaller`, or a test's stand-in for it: the one step that
 *   starts a process, and so the one a suite may not run for real.
 */
async function run(
  config,
  env = process.env,
  log = console.log,
  platform = process.platform,
  arch = process.arch,
  handOff = openInstaller,
) {
  if (config.mode === 'off') return
  if (isInstalled(platform, env)) return

  const suffix = installerSuffix(platform, arch)
  if (suffix === undefined || !hasDesktop(platform, env)) {
    log(`${PREFIX} no installer to run on ${platform}/${arch}; download one from ${RELEASES_PAGE}`)
    return
  }

  const state = readState(env)
  const forced = env.DSH_TAURIAPP_FORCE === '1'
  if (config.mode === 'notice' || (state.installer !== undefined && !forced)) {
    // Either this is a machine that only wants to be told, or it has already
    // been told once. Both end the same way: point at the release.
    log(`${PREFIX} the desktop app is not installed; get it from ${RELEASES_PAGE}`)
    if (typeof state.installer === 'string') log(`${PREFIX} an installer was downloaded and verified earlier: ${state.installer}`)
    return
  }

  const release = await fetchRelease(env.DSH_TAURIAPP_RELEASES_API || DEFAULT_RELEASES_API)
  const installer = assetNamed(release, (assetName) => assetName.endsWith(suffix))
  if (installer === undefined) throw new Error(`release ${release.version} carries no ${suffix} asset`)
  // Refused before the name is a path component, an opener argument or a
  // `SHA256SUMS` key: an asset named `../../x_amd64.deb` would write itself out
  // of the updates directory *and* verify, because the digest is looked up under
  // the same escaped name. Nothing is fetched for it either.
  if (!isBareFileName(installer.name)) {
    throw new Error(`release ${release.version} names its installer ${JSON.stringify(installer.name)}, which is not a bare file name`)
  }
  const checksums = assetNamed(release, (assetName) => assetName === CHECKSUMS)
  if (checksums === undefined) throw new Error(`release ${release.version} carries no ${CHECKSUMS}`)

  const sums = parseSha256Sums((await download(checksums.url)).toString('utf8'))
  const expected = sums.get(installer.name)
  if (expected === undefined) throw new Error(`${CHECKSUMS} does not list ${installer.name}`)

  log(`${PREFIX} downloading ${installer.name} (${release.version})`)
  const bytes = await download(installer.url)
  const { actual, ok } = verifyDigest(bytes, expected)
  if (!ok) throw new Error(`${installer.name} failed verification: SHA256SUMS says ${expected}, the download is ${actual}`)

  const path = installerPath(installer.name, env)
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, bytes)
  // The installers of the versions this one supersedes go, rather than
  // accumulating one per release for as long as the machine lives.
  for (const stale of pruneInstallers(dirname(path), installer.name)) {
    log(`${PREFIX} removed the superseded installer ${stale}`)
  }
  writeState({ version: release.version, installer: path, at: new Date().toISOString() }, env)
  log(`${PREFIX} verified ${installer.name} against ${CHECKSUMS}: ${path}`)

  if (!config.open || env.DSH_TAURIAPP_NO_OPEN === '1') {
    log(`${PREFIX} open it yourself with: ${manualCommand(path, platform)}`)
    logWslHint(log, platform, env)
    return
  }
  handOff(path, platform, log)
  log(`${PREFIX} handed the installer to the system; finish it there`)
  if (platform === 'linux') {
    // Not a footnote on Linux: the opener routinely completes with nothing
    // visible, and a `.deb` needs root whoever opens it, so this is the line that
    // works. On macOS a non-zero `open` is reported by `openInstaller` itself; on
    // Windows `start` answers nothing, so the verified path logged above is what a
    // person has to work from.
    log(`${PREFIX} if nothing opened: ${manualCommand(path, platform)}`)
    logWslHint(log, platform, env)
  }
}

/**
 * Report a failure the way the entry point does: what went wrong, then where to
 * get the application by hand.
 *
 * `describeError` rather than `error.message`, because the reason a `fetch`
 * failed lives in its `cause` — DNS, TLS, a proxy, a refused connection — and a
 * log line that says only `fetch failed` asks the reader to guess which of those
 * it was.
 * @param error - whatever `run` threw.
 * @param log - where to report.
 */
function reportFailure(error, log = console.log) {
  log(`${PREFIX} ${describeError(error)}`)
  log(`${PREFIX} download it manually from ${RELEASES_PAGE}`)
}

/**
 * Cordis entry point. A market stub must not be able to stop a harness from
 * booting, so the work is deferred — `apply` returns the promise the tests await,
 * and a failure ends as two log lines rather than as a rejected boot.
 * @param ctx - the Cordis context; deliberately unused.
 * @param config - the patch row's `config`.
 * @returns a promise that never rejects.
 */
export function apply(ctx, config) {
  return run(resolveConfig(config)).catch((error) => reportFailure(error))
}

/** Test surface: pure helpers plus the driver, none of it part of the contract. */
export const internals = {
  APP,
  API_TIMEOUT,
  CHECKSUMS,
  DEFAULT_CONFIG,
  DEFAULT_RELEASES_API,
  DOWNLOAD_TIMEOUT,
  INSTALLER_SUFFIXES,
  MODES,
  RELEASES_PAGE,
  STATE_FILE: 'plugin.json',
  assetNamed,
  describeError,
  download,
  dshHome,
  fetchRelease,
  hasDesktop,
  installedCandidates,
  installerPath,
  installerSuffix,
  isBareFileName,
  isInstalled,
  isTimeout,
  isWsl,
  manualCommand,
  openInstaller,
  openerCommand,
  parseSha256Sums,
  pruneInstallers,
  readState,
  reportFailure,
  resolveConfig,
  run,
  statePath,
  verifyDigest,
  withTimeout,
  writeState,
}

export { name }
