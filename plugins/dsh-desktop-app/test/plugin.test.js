/**
 * The stub's tests: the pure decisions (which asset, which mode, which name is
 * usable), the whole driver against a stand-in release server on loopback, and
 * the hand-off against a child that only records what it was handed. Nothing here
 * reaches GitHub, nothing here installs anything, and no case touches a real
 * platform opener.
 */

import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { EventEmitter } from 'node:events'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'

import { apply, internals, name as pluginName } from '../lib/index.js'

const { APP } = internals
const root = mkdtempSync(join(tmpdir(), 'dsh-xswt-tauriapp-test-'))
after(() => rmSync(root, { recursive: true, force: true }))

/** A temp `$DSH_HOME` inside the test root. */
const home = (label) => join(root, label)

/**
 * The half of a fixture host that says "nothing is installed here". The probe
 * looks in the directories the caller names, so a fixture names an empty one:
 * whether the machine running the suite has the application installed stops
 * mattering, which is the point of the parameter rather than a side effect.
 */
const notInstalled = () => ({ DSH_TAURIAPP_INSTALL_DIRS: join(root, 'nothing-installed') })

/**
 * A stand-in for the release API and its assets: one JSON release, one
 * `SHA256SUMS`, one installer, and a request log so a test can prove what was
 * and was not fetched.
 * @param sums - `'good'` to publish the real digest, anything else to publish a lie.
 * @param platform - the host the stand-in publishes an installer for.
 * @param arch - the host's architecture.
 * @param options - how to bend the release:
 *   `assetName` publishes the installer under a chosen name (a test uses one that
 *   is not a bare file name), `installerUrl` says where its bytes are served, and
 *   `stall` names a route prefix that accepts the connection and never answers.
 */
async function standIn(sums = 'good', platform = process.platform, arch = process.arch, options = {}) {
  const bytes = Buffer.from('a stand-in installer, never executed\n')
  const digest = createHash('sha256').update(bytes).digest('hex')
  const suffix = internals.installerSuffix(platform, arch)
  const installerName = options.assetName ?? `${APP}_0.0.99${suffix}`
  const server = createServer()
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const base = `http://127.0.0.1:${server.address().port}`
  // An asset name that is not a file name cannot be a route either, and the
  // download URL is whatever the feed claims it is.
  const installerUrl = options.installerUrl ?? `${base}/assets/${installerName}`
  const installerRoute = installerUrl.slice(base.length)
  const requests = []
  server.on('request', (request, response) => {
    requests.push(request.url)
    if (options.stall !== undefined && request.url.startsWith(options.stall)) {
      // Deliberately unanswered: this is the stalled connection a deadline exists
      // to turn into a failure. `closeAllConnections` lets the suite finish.
      return
    }
    if (request.url === '/releases/latest') {
      response.setHeader('content-type', 'application/json')
      response.end(
        JSON.stringify({
          tag_name: 'v0.0.99',
          html_url: `${base}/release`,
          assets: [
            { name: installerName, browser_download_url: installerUrl },
            { name: internals.CHECKSUMS, browser_download_url: `${base}/assets/${internals.CHECKSUMS}` },
          ],
        }),
      )
      return
    }
    if (request.url === `/assets/${internals.CHECKSUMS}`) {
      const published = sums === 'good' ? digest : 'f'.repeat(64)
      // The installer's own line, keyed by the name the release gave it — the
      // reason a digest can never be the check on an escaped name.
      response.end(`${published}  ${installerName}\n`)
      return
    }
    if (request.url === installerRoute) {
      response.end(bytes)
      return
    }
    response.statusCode = 404
    response.end('not found')
  })
  return {
    api: `${base}/releases/latest`,
    installerName,
    bytes,
    requests,
    // The path a write of this asset would land on. A name this plugin refuses
    // throws here rather than returning a path, which is its own assertion.
    installerPathFor: (env) => internals.installerPath(installerName, env),
    close: () =>
      new Promise((resolve) => {
        // undici keeps its sockets alive, so `close` alone would wait forever.
        server.closeAllConnections()
        server.close(resolve)
      }),
  }
}

/**
 * A real child process that stands in for the platform opener: it writes the argv
 * it was handed and exits with `code`. The program is Node running a script in the
 * test's own directory, so the hand-off is exercised — spawn, arguments, detached
 * options, exit code — without anything that could install or open on the host.
 * @param label - makes the recorder and its record file unique per case.
 * @param code - the exit status the stand-in opener ends with.
 */
function recordingOpener(label, code = 0) {
  const script = join(root, `opener-${label}.mjs`)
  const recordFile = join(root, `opener-${label}.argv.json`)
  writeFileSync(
    script,
    `import { writeFileSync } from 'node:fs'\n` +
      `writeFileSync(${JSON.stringify(recordFile)}, JSON.stringify(process.argv.slice(2)))\n` +
      `process.exit(${code})\n`,
  )
  const calls = []
  let reportExit
  const exited = new Promise((resolve) => {
    reportExit = resolve
  })
  const spawnImpl = (file, args, options) => {
    calls.push({ file, args, options })
    const child = spawn(process.execPath, [script, ...args], { ...options })
    // `openInstaller` unrefs the child, and an unref'd child on an otherwise idle
    // loop would let the process exit before the hand-off was observed.
    const keepAlive = setInterval(() => {}, 50)
    child.once('exit', (status) => {
      clearInterval(keepAlive)
      reportExit({ file, status })
    })
    return child
  }
  return { calls, exited, recordFile, spawnImpl }
}

/**
 * An opener that never starts: `spawn` fails asynchronously, which is the shape of
 * a machine with no `xdg-open` on it. Synthetic rather than real because the point
 * is the error path, and a missing-program name is only missing by accident.
 */
function unrunnableOpener() {
  const calls = []
  let reportError
  const errored = new Promise((resolve) => {
    reportError = resolve
  })
  const spawnImpl = (file, args, options) => {
    calls.push({ file, args, options })
    const child = new EventEmitter()
    child.unref = () => {}
    child.ref = () => {}
    const error = Object.assign(new Error(`spawn ${file} ENOENT`), { code: 'ENOENT', path: file })
    // `spawn` reports a missing program asynchronously, after the caller has
    // attached its handlers — which is the only reason this is a tick away.
    process.nextTick(() => {
      child.emit('error', error)
      reportError(error)
    })
    return child
  }
  return { calls, errored, spawnImpl }
}

/** A loopback port nothing is listening on: the way an offline machine fails. */
async function closedLoopback() {
  const server = createServer()
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()
  await new Promise((resolve) => server.close(resolve))
  return `http://127.0.0.1:${port}/releases/latest`
}

describe('the platform contract', () => {
  it('names the asset each platform installs from', () => {
    assert.equal(internals.installerSuffix('win32', 'x64'), '_x64-setup.exe')
    assert.equal(internals.installerSuffix('darwin', 'arm64'), '_aarch64.dmg')
    assert.equal(internals.installerSuffix('darwin', 'x64'), '_x64.dmg')
    assert.equal(internals.installerSuffix('linux', 'x64'), '_amd64.deb')
    // Nothing is published for these, and a guess would hand over the wrong
    // architecture's installer rather than none at all.
    assert.equal(internals.installerSuffix('linux', 'arm64'), undefined)
    assert.equal(internals.installerSuffix('win32', 'arm64'), undefined)
  })

  it('only offers an installer where a desktop could open one', () => {
    assert.equal(internals.hasDesktop('darwin', {}), true)
    assert.equal(internals.hasDesktop('linux', { DISPLAY: ':0' }), true)
    assert.equal(internals.hasDesktop('linux', { WAYLAND_DISPLAY: 'wayland-0' }), true)
    assert.equal(internals.hasDesktop('linux', {}), false)
    assert.equal(internals.hasDesktop('linux', { DISPLAY: ':0', CI: 'true' }), false)
  })

  it('looks for an installed copy where the host it names would have put it', () => {
    const windows = internals.installedCandidates('win32', {
      LOCALAPPDATA: 'C:\\Users\\u\\AppData\\Local',
      ProgramFiles: 'C:\\Program Files',
    })
    assert.ok(windows.length > 0)
    assert.ok(windows.every((candidate) => candidate.endsWith(`${APP}.exe`)))
    assert.ok(windows.some((candidate) => candidate.includes('Programs')))

    // One code path for a fixture and for the real environment: the directories
    // come from `DSH_TAURIAPP_INSTALL_DIRS`, the shape of what is in them comes
    // from the platform. Before, the branch was chosen by whether `env` *was*
    // `process.env`, so the tested path could only ever speak of `$HOME/.local/bin`
    // while every real host probed `/usr/bin` and `/Applications`.
    const fixtureDirs = [join(root, 'fake-bin'), join(root, 'fake-root')].join(delimiter)
    assert.deepEqual(internals.installedCandidates('linux', { DSH_TAURIAPP_INSTALL_DIRS: fixtureDirs }), [
      join(root, 'fake-bin', APP),
      join(root, 'fake-root', APP),
    ])
    // A macOS host keeps its bundle suffix in the same directories, and Windows
    // keeps its installed folder and `.exe`, because the platform decides that,
    // not the caller's identity.
    assert.deepEqual(internals.installedCandidates('darwin', { DSH_TAURIAPP_INSTALL_DIRS: fixtureDirs }), [
      join(root, 'fake-bin', `${APP}.app`),
      join(root, 'fake-root', `${APP}.app`),
    ])
    assert.deepEqual(internals.installedCandidates('win32', { DSH_TAURIAPP_INSTALL_DIRS: join(root, 'fake-winstalls') }), [
      join(root, 'fake-winstalls', APP, `${APP}.exe`),
    ])
    // A home bin is probeable on either unix, and only from the `HOME` given.
    assert.deepEqual(
      internals.installedCandidates('darwin', { DSH_TAURIAPP_INSTALL_DIRS: join(root, 'fake-apps'), HOME: '/home/u' }),
      [join(root, 'fake-apps', `${APP}.app`), join('/home/u', '.local', 'bin', APP)],
    )
    assert.deepEqual(internals.installedCandidates('linux', {}), [
      join('/usr/bin', APP),
      join('/usr/local/bin', APP),
      join('/opt', APP, APP),
    ])
    // A host that really has it, named the same way: no separate branch.
    assert.deepEqual(
      internals.installedCandidates('linux', { DSH_TAURIAPP_INSTALL_DIRS: '/usr/bin', HOME: '/home/u' }),
      [join('/usr/bin', APP), join('/home/u', '.local', 'bin', APP)],
    )
    // The real environment keeps the real defaults.
    assert.ok(internals.installedCandidates('linux', process.env).includes(join('/usr/bin', APP)))
    assert.ok(internals.installedCandidates('darwin', process.env).includes(join('/Applications', `${APP}.app`)))
  })

  it('prints a command that works when nothing opens by itself', () => {
    assert.match(internals.manualCommand('/tmp/x.deb', 'linux'), /apt install/)
    assert.match(internals.manualCommand('/tmp/x.dmg', 'darwin'), /^open /)
    assert.match(internals.manualCommand('C:\\x.exe', 'win32'), /^start /)
  })

  it('names the opener each platform is handed the file through', () => {
    // The constructed call, pinned for every platform rather than only the one
    // running the suite: `openInstaller` is the only place a process starts.
    assert.deepEqual(internals.openerCommand('/tmp/x_amd64.deb', 'win32'), {
      file: 'cmd.exe',
      args: ['/c', 'start', '', '/tmp/x_amd64.deb'],
    })
    assert.deepEqual(internals.openerCommand('/tmp/x_x64.dmg', 'darwin'), { file: 'open', args: ['/tmp/x_x64.dmg'] })
    assert.deepEqual(internals.openerCommand('/tmp/x_amd64.deb', 'linux'), {
      file: 'xdg-open',
      args: ['/tmp/x_amd64.deb'],
    })
  })

  it('recognises WSL, from the environment or from the host itself', () => {
    assert.equal(internals.isWsl({ WSL_DISTRO_NAME: 'Ubuntu' }), true)
    assert.equal(internals.isWsl({ WSL_INTEROP: '/run/WSL/1_interop' }), true)
    // A fixture describes a host, so the `/proc/version` fallback must not answer
    // for it — otherwise this suite would pass on WSL and fail everywhere else.
    assert.equal(internals.isWsl({}), false)
    assert.equal(internals.isWsl({ DSH_HOME: '/tmp/home', DISPLAY: ':0' }), false)
  })
})

describe('a release asset name', () => {
  it('is only usable as a bare file name', () => {
    // The names a real release carries.
    for (const usable of [
      `${APP}_0.0.99_amd64.deb`,
      `${APP}_0.0.99_x64-setup.exe`,
      `${APP}_0.0.99_x64.dmg`,
      internals.CHECKSUMS,
    ]) {
      assert.equal(internals.isBareFileName(usable), true, `${usable} is a file name`)
    }
    // Everything a feed could spell instead: the same name is a path component
    // under `$DSH_HOME`, an argument to the platform opener, and the key looked up
    // in `SHA256SUMS`, so an escaped one would both escape and verify.
    for (const unusable of [
      '../../escape_amd64.deb',
      '..\\escape_x64-setup.exe',
      '/etc/passwd_amd64.deb',
      'sub/dir/x_amd64.deb',
      'C:\\Windows\\x_x64-setup.exe',
      'x:alt_amd64.deb',
      '..',
      '.',
      '',
      'x\u0000_amd64.deb',
      'x\n_amd64.deb',
      'x\u007f_amd64.deb',
      undefined,
      42,
    ]) {
      assert.equal(internals.isBareFileName(unusable), false, `${JSON.stringify(unusable)} is not a file name`)
    }
  })

  it('is refused before it becomes a path', () => {
    const env = { DSH_HOME: home('name-refused') }
    assert.throws(() => internals.installerPath('../../escape_amd64.deb', env), /not a bare file name/)
    assert.equal(existsSync(join(home('name-refused'), APP, 'updates')), false)
  })

  it('stops a traversal the release would have vouched for, and writes nothing', async () => {
    // `DSH_TAURIAPP_RELEASES_API` is a documented override, so this JSON need not
    // come from GitHub. Here it names the installer with a path in it, and lists
    // that same escaped name in `SHA256SUMS` with the digest of the bytes served,
    // so the download would verify all the way while writing outside the updates
    // directory. The name is the only thing that can stop it.
    const escaped = `../../escape_${APP}_amd64.deb`
    const release = await standIn('good', 'linux', 'x64', { assetName: escaped })
    const env = {
      ...notInstalled(),
      DSH_HOME: home('traversal'),
      DISPLAY: ':0',
      DSH_TAURIAPP_RELEASES_API: release.api,
      DSH_TAURIAPP_NO_OPEN: '1',
    }
    try {
      await assert.rejects(
        internals.run({ mode: 'auto', open: true }, env, () => {}, 'linux', 'x64'),
        /not a bare file name/,
      )
      // Nothing else was fetched either: the name is refused at the release, not
      // at the moment of writing, so the checksum download never happens.
      assert.deepEqual(release.requests, ['/releases/latest'])
      // And nothing was written: not the updates directory, not the file the name
      // climbs to, not the state record.
      assert.equal(existsSync(home('traversal')), false)
      assert.equal(existsSync(join(home('traversal'), `escape_${APP}_amd64.deb`)), false)
      assert.equal(existsSync(join(root, `escape_${APP}_amd64.deb`)), false)
      assert.equal(existsSync(internals.statePath(env)), false)
    } finally {
      await release.close()
    }
  })
})

describe('requests that run out of time', () => {
  it('mirrors the shell\'s own deadlines', () => {
    // `API_TIMEOUT` / `DOWNLOAD_TIMEOUT` in `crates/dsh-core/src/self_update.rs`:
    // the same questions, so the same patience.
    assert.equal(internals.API_TIMEOUT, 15_000)
    assert.equal(internals.DOWNLOAD_TIMEOUT, 300_000)
  })

  it('calls a stalled release lookup a failure', async () => {
    const release = await standIn('good', 'linux', 'x64', { stall: '/releases/latest' })
    try {
      const error = await internals.fetchRelease(release.api, 150).catch((thrown) => thrown)
      assert.ok(error instanceof Error)
      assert.equal(error.message, 'the release lookup did not answer within 0.15s')
      // The deadline is the reason, and it is kept rather than dropped.
      assert.equal(internals.isTimeout(error.cause), true)
      assert.match(internals.describeError(error), /did not answer within/)
    } finally {
      await release.close()
    }
  })

  it('calls a stalled or half-sent download a failure', async () => {
    // The release lookup answers; the asset route never does. A body that stops
    // streaming counts against the same deadline, which is why the whole request
    // including `arrayBuffer` sits inside it.
    const release = await standIn('good', 'linux', 'x64', { stall: '/assets/' })
    try {
      const fetched = await internals.fetchRelease(release.api, 1_000)
      assert.equal(fetched.version, '0.0.99')
      const error = await internals.download(fetched.assets[0].url, 150).catch((thrown) => thrown)
      assert.equal(error.message, 'the download did not answer within 0.15s')
      assert.equal(internals.isTimeout(error.cause), true)
    } finally {
      await release.close()
    }
  })

  it('recognises a deadline in either shape Node throws it in', () => {
    const timeout = Object.assign(new Error('The operation was aborted due to timeout'), { name: 'TimeoutError' })
    assert.equal(internals.isTimeout(timeout), true)
    assert.equal(internals.isTimeout(new Error('fetch failed', { cause: timeout })), true)
    assert.equal(internals.isTimeout(new Error('the download answered HTTP 500')), false)
    assert.equal(internals.isTimeout('TimeoutError'), false)
    assert.equal(internals.isTimeout(null), false)
  })

  it('reaches the log as a failure, not as a hang', async () => {
    const release = await standIn('good', 'linux', 'x64', { stall: '/releases/latest' })
    const lines = []
    try {
      const error = await internals.fetchRelease(release.api, 120).catch((thrown) => thrown)
      internals.reportFailure(error, (line) => lines.push(line))
      assert.ok(lines.some((line) => /the release lookup did not answer within/.test(line)))
      assert.ok(lines.some((line) => line.includes(internals.RELEASES_PAGE)))
    } finally {
      await release.close()
    }
  })
})

describe('a failure keeps its reason', () => {
  it('reads the cause out of a real fetch failure', async () => {
    // Loopback, a port that was open a moment ago and is not now: the way
    // `fetch failed` arrives with the reason one level down.
    const error = await internals.fetchRelease(await closedLoopback()).catch((thrown) => thrown)
    assert.equal(error.message, 'fetch failed')
    assert.match(internals.describeError(error), /ECONNREFUSED|refused/i)
    assert.notEqual(internals.describeError(error), error.message)
  })

  it('follows a cause chain and never invents one', () => {
    const dns = Object.assign(new Error('getaddrinfo ENOTFOUND api.github.com'), { code: 'ENOTFOUND' })
    const tls = new Error('unable to verify the first certificate', { cause: dns })
    const fetchFailed = new TypeError('fetch failed', { cause: tls })
    assert.equal(
      internals.describeError(fetchFailed),
      'fetch failed: unable to verify the first certificate: getaddrinfo ENOTFOUND api.github.com',
    )
    // A cause that is not an `Error` still reads, and an empty one adds nothing.
    assert.equal(internals.describeError(new Error('fetch failed', { cause: 'proxy refused' })), 'fetch failed: proxy refused')
    assert.equal(internals.describeError(new Error('fetch failed', { cause: null })), 'fetch failed')
    assert.equal(internals.describeError(new Error('plain')), 'plain')
    // An `Error` with no message is still named.
    assert.equal(internals.describeError(new TypeError('')), 'TypeError')
    assert.equal(internals.describeError('a string'), 'a string')
    // The chain is finite however deep it nests, so a cause cannot loop forever.
    let deep = new Error('level 0')
    for (let index = 1; index <= 8; index += 1) deep = new Error(`level ${index}`, { cause: deep })
    assert.equal(internals.describeError(deep).split(': ').length, 4)
  })

  it('is reported by the entry point as two lines, the reason and the page', () => {
    const lines = []
    internals.reportFailure(new TypeError('fetch failed', { cause: new Error('connect ECONNREFUSED 127.0.0.1:1') }), (line) =>
      lines.push(line),
    )
    assert.deepEqual(lines, [
      `[dsh-xswt-tauriapp] fetch failed: connect ECONNREFUSED 127.0.0.1:1`,
      `[dsh-xswt-tauriapp] download it manually from ${internals.RELEASES_PAGE}`,
    ])
  })
})

describe('the hand-off', () => {
  it('hands the verified path to the opener, detached and unwaited', async () => {
    for (const platform of ['win32', 'darwin', 'linux']) {
      const opener = recordingOpener(`deliver-${platform}`)
      const lines = []
      internals.openInstaller(join(root, `installer.${platform}`), platform, (line) => lines.push(line), opener.spawnImpl)

      assert.equal(opener.calls.length, 1, `${platform} starts exactly one process`)
      const { file, args, options } = opener.calls[0]
      assert.deepEqual({ file, args }, internals.openerCommand(join(root, `installer.${platform}`), platform))
      // An installer window outlives the plugin, so the child is detached and its
      // stdio is not a pipe back to a harness that is already moving on.
      assert.equal(options.detached, true)
      assert.equal(options.stdio, 'ignore')
      assert.equal(options.windowsHide, true)

      await opener.exited
      // What the child was really handed, in order, with the path whole.
      assert.deepEqual(JSON.parse(readFileSync(opener.recordFile, 'utf8')), args)
      assert.deepEqual(lines, [], 'a hand-off that worked is not a log line')
    }
  })

  it('says so when the opener exits without opening anything', async () => {
    // `xdg-open` with no handler for `.deb`: it runs, it exits 3, nothing opens.
    const opener = recordingOpener('exit-3', 3)
    const lines = []
    internals.openInstaller('/tmp/x_amd64.deb', 'linux', (line) => lines.push(line), opener.spawnImpl)
    const { status } = await opener.exited

    assert.equal(status, 3)
    assert.ok(
      lines.some((line) => line.includes('xdg-open exited 3') && line.includes('sudo apt install')),
      `the failure and the way around it: ${JSON.stringify(lines)}`,
    )
  })

  it('says so when the opener cannot be run at all', async () => {
    const opener = unrunnableOpener()
    const lines = []
    internals.openInstaller('/tmp/x_amd64.deb', 'linux', (line) => lines.push(line), opener.spawnImpl)
    await opener.errored
    assert.equal(opener.calls.length, 1, 'the attempt is made once')
    assert.equal(opener.calls[0].file, 'xdg-open')
    assert.ok(
      lines.some((line) => /could not run xdg-open/.test(line)),
      `a machine with no opener is told, not left waiting: ${JSON.stringify(lines)}`,
    )
  })
})

describe('reading the release', () => {
  it('parses sha256sum output and ignores anything else', () => {
    const sums = internals.parseSha256Sums(
      [
        `${'a'.repeat(64)}  dsh-xswt-tauriapp_0.0.7_amd64.deb`,
        `${'b'.repeat(64)} *dsh-xswt-tauriapp_0.0.7_x64-setup.exe`,
        `${'c'.repeat(64)}  SHA256SUMS`,
        'not a checksum line',
        `${'d'.repeat(63)}  too-short.deb`,
        '',
      ].join('\n'),
    )
    assert.equal(sums.get('dsh-xswt-tauriapp_0.0.7_amd64.deb'), 'a'.repeat(64))
    assert.equal(sums.get('dsh-xswt-tauriapp_0.0.7_x64-setup.exe'), 'b'.repeat(64))
    assert.equal(sums.size, 3)
  })

  it('accepts only the digest the release published', () => {
    const bytes = Buffer.from('installer')
    const digest = createHash('sha256').update(bytes).digest('hex')
    assert.deepEqual(internals.verifyDigest(bytes, digest.toUpperCase()), { actual: digest, ok: true })
    assert.equal(internals.verifyDigest(bytes, '0'.repeat(64)).ok, false)
    assert.equal(internals.verifyDigest(Buffer.from('installer '), digest).ok, false)
  })
})

describe('configuration', () => {
  it('defaults to downloading and opening, and lets the environment win', () => {
    assert.deepEqual(internals.resolveConfig(undefined), { mode: 'auto', open: true })
    assert.deepEqual(internals.resolveConfig({ mode: 'notice', open: false }), { mode: 'notice', open: false })
    assert.deepEqual(internals.resolveConfig({ mode: 'nonsense' }), { mode: 'auto', open: true })

    process.env.DSH_TAURIAPP_MODE = 'off'
    try {
      assert.equal(internals.resolveConfig({ mode: 'auto' }).mode, 'off')
    } finally {
      delete process.env.DSH_TAURIAPP_MODE
    }
  })

  it('keeps its state under $DSH_HOME', () => {
    assert.equal(internals.statePath({ DSH_HOME: '/tmp/home' }), join('/tmp/home', APP, 'plugin.json'))
    assert.equal(internals.readState({ DSH_HOME: join(root, 'empty') }).installer, undefined)
  })

  it('keeps the installer it downloads under $DSH_HOME, not in a temporary directory', () => {
    // Both the recorded state and the printed `sudo apt install <path>` name
    // this file, and the natural thing to do with that line is run it later — by
    // which time a temporary directory has been emptied.
    assert.equal(
      internals.installerPath(`${APP}_0.0.99_amd64.deb`, { DSH_HOME: '/tmp/home' }),
      join('/tmp/home', APP, 'updates', `${APP}_0.0.99_amd64.deb`),
    )
  })

  it('removes the installer a newer download supersedes, and nothing else', () => {
    const env = { DSH_HOME: home('prune') }
    const older = internals.installerPath(`${APP}_0.0.98_amd64.deb`, env)
    const keep = internals.installerPath(`${APP}_0.0.99_amd64.deb`, env)
    const foreign = join(dirname(older), 'notes.txt')
    mkdirSync(dirname(older), { recursive: true })
    for (const path of [older, keep, foreign]) writeFileSync(path, 'x')

    const removed = internals.pruneInstallers(dirname(older), `${APP}_0.0.99_amd64.deb`)

    assert.deepEqual(removed, [`${APP}_0.0.98_amd64.deb`])
    assert.equal(existsSync(older), false)
    assert.equal(existsSync(keep), true)
    assert.equal(existsSync(foreign), true, 'a file that is not one of ours is not touched')
  })

  it('prunes exactly the suffixes it can download', () => {
    // The two lists are the same knowledge in two places, so a new bundle format
    // must not become an installer that is never cleaned up.
    for (const [platform, arch] of [
      ['win32', 'x64'],
      ['darwin', 'arm64'],
      ['darwin', 'x64'],
      ['linux', 'x64'],
    ]) {
      const suffix = internals.installerSuffix(platform, arch)
      assert.ok(
        internals.INSTALLER_SUFFIXES.includes(suffix),
        `${platform}/${arch} publishes ${suffix}, which pruning does not know`,
      )
    }
  })
})

describe('a first start after installation', () => {
  it('downloads, verifies, records, and stays quiet afterwards', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = {
      ...notInstalled(),
      DSH_HOME: home('first'),
      DISPLAY: ':0',
      DSH_TAURIAPP_RELEASES_API: release.api,
      DSH_TAURIAPP_NO_OPEN: '1',
    }
    const lines = []
    try {
      await internals.run({ mode: 'auto', open: true }, env, (line) => lines.push(line), 'linux', 'x64')

      assert.deepEqual(release.requests, [
        '/releases/latest',
        `/assets/${internals.CHECKSUMS}`,
        `/assets/${release.installerName}`,
      ])
      assert.deepEqual(readFileSync(release.installerPathFor(env)), release.bytes)
      const state = JSON.parse(readFileSync(internals.statePath(env), 'utf8'))
      assert.equal(state.version, '0.0.99')
      assert.equal(state.installer, release.installerPathFor(env))
      assert.ok(lines.some((line) => line.includes(`verified ${release.installerName}`)))

      // The second start has nothing to do: it must not fetch the release again,
      // and it must still say where the earlier download went.
      const second = []
      const before = release.requests.length
      await internals.run({ mode: 'auto', open: true }, env, (line) => second.push(line), 'linux', 'x64')
      assert.equal(release.requests.length, before)
      assert.ok(second.some((line) => line.includes(release.installerPathFor(env))))
    } finally {
      rmSync(release.installerPathFor(env), { force: true })
      await release.close()
    }
  })

  it('carries a Windows host all the way to the hand-off', async () => {
    // The shape no other case took: a win32 host that reaches the hand-off rather
    // than stopping at `DSH_TAURIAPP_NO_OPEN`. The hand-off is recorded, not run —
    // `openInstaller` itself is exercised against a stand-in child above, and a
    // suite must never start a real installer.
    const release = await standIn('good', 'win32', 'x64')
    const env = {
      ...notInstalled(),
      DSH_HOME: home('win32'),
      DSH_TAURIAPP_RELEASES_API: release.api,
    }
    const handed = []
    const lines = []
    try {
      await internals.run(
        { mode: 'auto', open: true },
        env,
        (line) => lines.push(line),
        'win32',
        'x64',
        (path, platform) => {
          handed.push({ path, platform })
        },
      )

      assert.deepEqual(handed, [{ path: release.installerPathFor(env), platform: 'win32' }])
      assert.deepEqual(readFileSync(release.installerPathFor(env)), release.bytes)
      assert.equal(JSON.parse(readFileSync(internals.statePath(env), 'utf8')).installer, release.installerPathFor(env))
      assert.ok(lines.some((line) => line.includes('handed the installer to the system')))
      // The Linux-only lines stay Linux-only.
      assert.equal(lines.some((line) => line.includes('WSL:')), false)
      assert.equal(lines.some((line) => line.includes('if nothing opened')), false)
    } finally {
      rmSync(release.installerPathFor(env), { force: true })
      await release.close()
    }
  })

  it('refuses a download the release did not vouch for, and writes nothing', async () => {
    const release = await standIn('bad', 'linux', 'x64')
    const env = {
      ...notInstalled(),
      DSH_HOME: home('bad'),
      DISPLAY: ':0',
      DSH_TAURIAPP_RELEASES_API: release.api,
      DSH_TAURIAPP_NO_OPEN: '1',
    }
    try {
      await assert.rejects(
        internals.run({ mode: 'auto', open: true }, env, () => {}, 'linux', 'x64'),
        /failed verification/,
      )
      assert.equal(existsSync(release.installerPathFor(env)), false)
      assert.equal(existsSync(internals.statePath(env)), false)
    } finally {
      await release.close()
    }
  })

  it('says where to look instead of downloading on a machine with no desktop', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = {
      ...notInstalled(),
      DSH_HOME: home('headless'),
      DSH_TAURIAPP_RELEASES_API: release.api,
    }
    const lines = []
    try {
      await internals.run({ mode: 'auto', open: true }, env, (line) => lines.push(line), 'linux', 'x64')
      assert.deepEqual(release.requests, [])
      assert.ok(lines.some((line) => line.includes(internals.RELEASES_PAGE)))
    } finally {
      await release.close()
    }
  })

  it('tells a WSL host how to finish a .deb, which xdg-open cannot install', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = {
      ...notInstalled(),
      DSH_HOME: home('wsl'),
      DISPLAY: ':0',
      WSL_DISTRO_NAME: 'Ubuntu',
      DSH_TAURIAPP_RELEASES_API: release.api,
      DSH_TAURIAPP_NO_OPEN: '1',
    }
    const lines = []
    try {
      await internals.run({ mode: 'auto', open: true }, env, (line) => lines.push(line), 'linux', 'x64')
      assert.ok(lines.some((line) => /apt install/.test(line)))
      assert.ok(
        lines.some((line) => /WSL:/.test(line)),
        'a WSL host is told why the automatic handoff cannot finish',
      )
    } finally {
      rmSync(release.installerPathFor(env), { force: true })
      await release.close()
    }
  })

  it('does not blame WSL when it is not on WSL', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = {
      ...notInstalled(),
      DSH_HOME: home('plain-linux'),
      DISPLAY: ':0',
      DSH_TAURIAPP_RELEASES_API: release.api,
      DSH_TAURIAPP_NO_OPEN: '1',
    }
    const lines = []
    try {
      await internals.run({ mode: 'auto', open: true }, env, (line) => lines.push(line), 'linux', 'x64')
      assert.ok(lines.some((line) => /apt install/.test(line)))
      assert.ok(!lines.some((line) => /WSL:/.test(line)))
    } finally {
      rmSync(release.installerPathFor(env), { force: true })
      await release.close()
    }
  })

  it('downloads nothing at all in notice mode', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = {
      ...notInstalled(),
      DSH_HOME: home('notice'),
      DISPLAY: ':0',
      DSH_TAURIAPP_RELEASES_API: release.api,
    }
    const lines = []
    try {
      await internals.run({ mode: 'notice', open: true }, env, (line) => lines.push(line), 'linux', 'x64')
      assert.deepEqual(release.requests, [])
      assert.ok(lines.some((line) => line.includes(internals.RELEASES_PAGE)))
    } finally {
      await release.close()
    }
  })

  it('says nothing at all when the application is already installed', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const fakeHome = home('installed-home')
    const installed = join(fakeHome, '.local', 'bin', APP)
    mkdirSync(dirname(installed), { recursive: true })
    writeFileSync(installed, '#!/bin/sh\n')

    const env = {
      HOME: fakeHome,
      DSH_TAURIAPP_INSTALL_DIRS: join(root, 'nothing-installed'),
      DSH_HOME: home('installed'),
      DISPLAY: ':0',
      DSH_TAURIAPP_RELEASES_API: release.api,
    }
    const lines = []
    try {
      await internals.run({ mode: 'auto', open: true }, env, (line) => lines.push(line), 'linux', 'x64')
      // The whole point of the check: a machine that has the shell is not asked
      // for anything, and is not told anything either.
      assert.deepEqual(release.requests, [])
      assert.deepEqual(lines, [])
    } finally {
      await release.close()
    }
  })

  it('finds an installed copy in a directory the host names, not only in a home', async () => {
    // The same probe, the other half: a fixture that puts the application where it
    // says its `PATH`-style install directory is. Under the old fork a fixture
    // could not express this at all.
    const release = await standIn('good', 'linux', 'x64')
    const fakeBin = join(root, 'installed-bin')
    mkdirSync(fakeBin, { recursive: true })
    writeFileSync(join(fakeBin, APP), '#!/bin/sh\n')
    const env = {
      DSH_HOME: home('named-dirs'),
      DISPLAY: ':0',
      DSH_TAURIAPP_INSTALL_DIRS: [fakeBin, join(root, 'nothing-installed')].join(delimiter),
      DSH_TAURIAPP_RELEASES_API: release.api,
    }
    const lines = []
    try {
      await internals.run({ mode: 'auto', open: true }, env, (line) => lines.push(line), 'linux', 'x64')
      assert.deepEqual(release.requests, [])
      assert.deepEqual(lines, [])
    } finally {
      await release.close()
    }
  })
})

describe('the cordis entry point', () => {
  it('is named, and turns a failed lookup into log lines rather than a rejected boot', async () => {
    assert.equal(pluginName, 'xswt-tauriapp')

    // Port 1 is nobody's listener, so this fails the way an offline machine
    // fails. A market stub that could reject here could stop a harness from
    // starting, which is the one thing it must never do.
    const saved = {
      DSH_HOME: process.env.DSH_HOME,
      DISPLAY: process.env.DISPLAY,
      CI: process.env.CI,
      HOME: process.env.HOME,
      DSH_TAURIAPP_INSTALL_DIRS: process.env.DSH_TAURIAPP_INSTALL_DIRS,
      DSH_TAURIAPP_RELEASES_API: process.env.DSH_TAURIAPP_RELEASES_API,
      DSH_TAURIAPP_MODE: process.env.DSH_TAURIAPP_MODE,
      DSH_TAURIAPP_NO_OPEN: process.env.DSH_TAURIAPP_NO_OPEN,
    }
    const lines = []
    const originalLog = console.log
    console.log = (line) => lines.push(String(line))
    process.env.DSH_HOME = home('apply')
    process.env.DISPLAY = ':0'
    // A host with nothing installed and no desktop question left to chance, so the
    // assertions below hold on every machine rather than only on ones where the
    // application happens to be absent.
    process.env.DSH_TAURIAPP_INSTALL_DIRS = join(root, 'nothing-installed')
    process.env.HOME = join(root, 'apply-home')
    delete process.env.CI
    delete process.env.DSH_TAURIAPP_NO_OPEN
    process.env.DSH_TAURIAPP_RELEASES_API = 'http://127.0.0.1:1/releases/latest'
    delete process.env.DSH_TAURIAPP_MODE
    try {
      await assert.doesNotReject(apply(undefined, { mode: 'auto' }), 'apply() rejected')
    } finally {
      console.log = originalLog
      for (const [key, value] of Object.entries(saved)) {
        if (value === undefined) delete process.env[key]
        else process.env[key] = value
      }
    }

    // Unconditional: the application is not installed on the host this describes,
    // so something is always said, and it is always a prefixed line that points at
    // the release — whether that is the download failing or the platform having no
    // installer to fetch.
    assert.ok(lines.length > 0, 'a stub that reaches nothing also logs nothing')
    for (const line of lines) assert.match(line, /^\[dsh-xswt-tauriapp] /)
    assert.ok(lines.some((line) => line.includes(internals.RELEASES_PAGE)))
  })
})
