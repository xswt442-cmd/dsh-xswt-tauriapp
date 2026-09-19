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
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

/** Stable Cordis plugin name. */
const name = 'xswt-tauriapp'

/** This repository's coordinates: the release lookup and the release page. */
const REPO = 'xswt442-cmd/dsh-xswt-tauriapp'
const DEFAULT_RELEASES_API = `https://api.github.com/repos/${REPO}/releases/latest`
const RELEASES_PAGE = `https://github.com/${REPO}/releases/latest`
const USER_AGENT = 'dsh-xswt-tauriapp-plugin'

/** The one asset every release carries beside its installers. */
const CHECKSUMS = 'SHA256SUMS'

/** The installed application's name, in its bundle and its executable. */
const APP = 'dsh-xswt-tauriapp'

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
 * Where the shell would already be if its installer had run. Best effort by
 * design: this only decides whether the plugin stays quiet, and a miss costs one
 * redundant download rather than a wrong action.
 * @param platform - `process.platform`.
 * @param env - the environment to read.
 * @returns candidate paths, any of which means "already installed".
 */
function installedCandidates(platform = process.platform, env = process.env) {
  if (platform === 'win32') {
    const roots = [
      env.LOCALAPPDATA,
      env.LOCALAPPDATA !== undefined ? join(env.LOCALAPPDATA, 'Programs') : undefined,
      env.ProgramFiles,
      env['ProgramFiles(x86)'],
    ]
    return roots.filter(Boolean).map((root) => join(root, APP, `${APP}.exe`))
  }
  if (platform === 'darwin') return [`/Applications/${APP}.app`]
  return [`/usr/bin/${APP}`, `/usr/local/bin/${APP}`, join(homedir(), '.local/bin', APP), `/opt/${APP}/${APP}`]
}

/** @returns whether the application is already installed. */
function isInstalled(platform = process.platform, env = process.env) {
  return installedCandidates(platform, env).some((candidate) => existsSync(candidate))
}

/**
 * Read the release the shell's own updater reads.
 * @param api - the releases API URL, `DSH_TAURIAPP_RELEASES_API` if set.
 * @returns the version, the release page and the asset list.
 */
async function fetchRelease(api) {
  const response = await fetch(api, {
    headers: { accept: 'application/vnd.github+json', 'user-agent': USER_AGENT },
  })
  if (!response.ok) throw new Error(`the release lookup answered HTTP ${response.status}`)
  const release = await response.json()
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
async function download(url) {
  const response = await fetch(url, { headers: { 'user-agent': USER_AGENT } })
  if (!response.ok) throw new Error(`the download answered HTTP ${response.status}`)
  return Buffer.from(await response.arrayBuffer())
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
 * Hand a verified file to the platform opener. Spawning a detached child is the
 * whole of it: the user answers SmartScreen, Gatekeeper or the package manager,
 * which is exactly the boundary this plugin promises.
 * @param path - the verified installer.
 * @param platform - `process.platform`.
 * @param log - where a failure to spawn is reported.
 */
function openInstaller(path, platform = process.platform, log = console.log) {
  const command =
    platform === 'win32'
      ? { file: 'cmd.exe', args: ['/c', 'start', '', path] }
      : platform === 'darwin'
        ? { file: 'open', args: [path] }
        : { file: 'xdg-open', args: [path] }
  try {
    const child = spawn(command.file, command.args, { detached: true, stdio: 'ignore', windowsHide: true })
    child.on('error', (error) => log(`${PREFIX} could not run ${command.file}: ${error.message}`))
    child.unref()
  } catch (error) {
    log(`${PREFIX} could not run ${command.file}: ${error instanceof Error ? error.message : String(error)}`)
  }
}

/**
 * The plugin's whole behaviour: notice the application is missing, fetch this
 * platform's installer, verify it, hand it over, remember that it happened.
 *
 * Every exit is a log line, and the only writes are the installer (after its
 * digest matched) and the state file under `$DSH_HOME`. Exported through
 * `internals` so the tests can drive it against a stand-in release server.
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
 */
async function run(config, env = process.env, log = console.log, platform = process.platform, arch = process.arch) {
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
  const checksums = assetNamed(release, (assetName) => assetName === CHECKSUMS)
  if (checksums === undefined) throw new Error(`release ${release.version} carries no ${CHECKSUMS}`)

  const sums = parseSha256Sums((await download(checksums.url)).toString('utf8'))
  const expected = sums.get(installer.name)
  if (expected === undefined) throw new Error(`${CHECKSUMS} does not list ${installer.name}`)

  log(`${PREFIX} downloading ${installer.name} (${release.version})`)
  const bytes = await download(installer.url)
  const { actual, ok } = verifyDigest(bytes, expected)
  if (!ok) throw new Error(`${installer.name} failed verification: SHA256SUMS says ${expected}, the download is ${actual}`)

  const path = join(tmpdir(), APP, 'updates', installer.name)
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, bytes)
  writeState({ version: release.version, installer: path, at: new Date().toISOString() }, env)
  log(`${PREFIX} verified ${installer.name} against ${CHECKSUMS}: ${path}`)

  if (!config.open || env.DSH_TAURIAPP_NO_OPEN === '1') {
    log(`${PREFIX} open it yourself with: ${manualCommand(path, platform)}`)
    return
  }
  openInstaller(path, platform, log)
  log(`${PREFIX} handed the installer to the system; finish it there (nothing opened? ${manualCommand(path)})`)
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
  return run(resolveConfig(config)).catch((error) => {
    console.log(`${PREFIX} ${error instanceof Error ? error.message : String(error)}`)
    console.log(`${PREFIX} download it manually from ${RELEASES_PAGE}`)
  })
}

/** Test surface: pure helpers plus the driver, none of it part of the contract. */
export const internals = {
  APP,
  CHECKSUMS,
  DEFAULT_CONFIG,
  DEFAULT_RELEASES_API,
  MODES,
  RELEASES_PAGE,
  STATE_FILE: 'plugin.json',
  assetNamed,
  dshHome,
  fetchRelease,
  hasDesktop,
  installedCandidates,
  installerSuffix,
  isInstalled,
  manualCommand,
  parseSha256Sums,
  readState,
  resolveConfig,
  run,
  statePath,
  verifyDigest,
  writeState,
}

export { name }
