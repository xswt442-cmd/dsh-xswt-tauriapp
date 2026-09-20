/**
 * The stub's tests: the pure decisions (which asset, which mode), and the whole
 * driver against a stand-in release server on loopback. Nothing here reaches
 * GitHub, and nothing here opens an installer — `DSH_TAURIAPP_NO_OPEN` keeps the
 * run at the point where it would have handed the file over.
 */

import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'

import { apply, internals, name as pluginName } from '../lib/index.js'

const { APP } = internals
const root = mkdtempSync(join(tmpdir(), 'dsh-xswt-tauriapp-test-'))
after(() => rmSync(root, { recursive: true, force: true }))

/** A temp `$DSH_HOME` inside the test root. */
const home = (label) => join(root, label)

/**
 * A stand-in for the release API and its assets: one JSON release, one
 * `SHA256SUMS`, one installer, and a request log so a test can prove what was
 * and was not fetched.
 * @param sums - `'good'` to publish the real digest, anything else to publish a lie.
 * @param platform - the host the stand-in publishes an installer for.
 * @param arch - the host's architecture.
 */
async function standIn(sums = 'good', platform = process.platform, arch = process.arch) {
  const bytes = Buffer.from('a stand-in installer, never executed\n')
  const digest = createHash('sha256').update(bytes).digest('hex')
  const suffix = internals.installerSuffix(platform, arch)
  const installerName = `${APP}_0.0.99${suffix}`
  const requests = []
  const server = createServer()
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const base = `http://127.0.0.1:${server.address().port}`
  server.on('request', (request, response) => {
    requests.push(request.url)
    if (request.url === '/releases/latest') {
      response.setHeader('content-type', 'application/json')
      response.end(
        JSON.stringify({
          tag_name: 'v0.0.99',
          html_url: `${base}/release`,
          assets: [
            { name: installerName, browser_download_url: `${base}/assets/${installerName}` },
            { name: internals.CHECKSUMS, browser_download_url: `${base}/assets/${internals.CHECKSUMS}` },
          ],
        }),
      )
      return
    }
    if (request.url === `/assets/${internals.CHECKSUMS}`) {
      const published = sums === 'good' ? digest : 'f'.repeat(64)
      response.end(`${published}  ${installerName}\n`)
      return
    }
    if (request.url === `/assets/${installerName}`) {
      response.end(bytes)
      return
    }
    response.statusCode = 404
    response.end('not found')
  })
  return {
    api: `${base}/releases/latest`,
    installerName,
    installerPath: join(tmpdir(), APP, 'updates', installerName),
    bytes,
    requests,
    close: () =>
      new Promise((resolve) => {
        // undici keeps its sockets alive, so `close` alone would wait forever.
        server.closeAllConnections()
        server.close(resolve)
      }),
  }
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

  it('looks for an installed copy where an installer would have put it', () => {
    const windows = internals.installedCandidates('win32', {
      LOCALAPPDATA: 'C:\\Users\\u\\AppData\\Local',
      ProgramFiles: 'C:\\Program Files',
    })
    assert.ok(windows.length > 0)
    assert.ok(windows.every((candidate) => candidate.endsWith(`${APP}.exe`)))
    assert.ok(windows.some((candidate) => candidate.includes('Programs')))

    // A fixture describes a host, so its own home is the whole filesystem it has;
    // the machine's real paths belong to the real environment, and asking the
    // fixture for them is what made these tests depend on whether the person
    // running them had installed the application.
    assert.deepEqual(internals.installedCandidates('linux', {}), [])
    assert.deepEqual(internals.installedCandidates('darwin', { HOME: '/home/u' }), [
      join('/home/u', '.local', 'bin', APP),
    ])
    assert.ok(internals.installedCandidates('linux', process.env).includes(`/usr/bin/${APP}`))
    assert.deepEqual(internals.installedCandidates('darwin', process.env), [`/Applications/${APP}.app`])
  })

  it('prints a command that works when nothing opens by itself', () => {
    assert.match(internals.manualCommand('/tmp/x.deb', 'linux'), /apt install/)
    assert.match(internals.manualCommand('/tmp/x.dmg', 'darwin'), /^open /)
    assert.match(internals.manualCommand('C:\\x.exe', 'win32'), /^start /)
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
})

describe('a first start after installation', () => {
  it('downloads, verifies, records, and stays quiet afterwards', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = {
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
      assert.deepEqual(readFileSync(release.installerPath), release.bytes)
      const state = JSON.parse(readFileSync(internals.statePath(env), 'utf8'))
      assert.equal(state.version, '0.0.99')
      assert.equal(state.installer, release.installerPath)
      assert.ok(lines.some((line) => line.includes(`verified ${release.installerName}`)))

      // The second start has nothing to do: it must not fetch the release again,
      // and it must still say where the earlier download went.
      const second = []
      const before = release.requests.length
      await internals.run({ mode: 'auto', open: true }, env, (line) => second.push(line), 'linux', 'x64')
      assert.equal(release.requests.length, before)
      assert.ok(second.some((line) => line.includes(release.installerPath)))
    } finally {
      rmSync(release.installerPath, { force: true })
      await release.close()
    }
  })

  it('refuses a download the release did not vouch for, and writes nothing', async () => {
    const release = await standIn('bad', 'linux', 'x64')
    const env = {
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
      assert.equal(existsSync(release.installerPath), false)
      assert.equal(existsSync(internals.statePath(env)), false)
    } finally {
      await release.close()
    }
  })

  it('says where to look instead of downloading on a machine with no desktop', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = { DSH_HOME: home('headless'), DSH_TAURIAPP_RELEASES_API: release.api }
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
      rmSync(release.installerPath, { force: true })
      await release.close()
    }
  })

  it('does not blame WSL when it is not on WSL', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = {
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
      rmSync(release.installerPath, { force: true })
      await release.close()
    }
  })

  it('downloads nothing at all in notice mode', async () => {
    const release = await standIn('good', 'linux', 'x64')
    const env = { DSH_HOME: home('notice'), DISPLAY: ':0', DSH_TAURIAPP_RELEASES_API: release.api }
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
      DSH_TAURIAPP_RELEASES_API: process.env.DSH_TAURIAPP_RELEASES_API,
      DSH_TAURIAPP_MODE: process.env.DSH_TAURIAPP_MODE,
    }
    const lines = []
    const originalLog = console.log
    console.log = (line) => lines.push(String(line))
    process.env.DSH_HOME = home('apply')
    process.env.DISPLAY = ':0'
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

    // Nothing is logged when the application is already installed, which is the
    // only other way this can end; a CI machine and a fresh profile take the
    // first branch.
    for (const line of lines) assert.match(line, /^\[dsh-xswt-tauriapp] /)
    if (lines.length > 0) assert.ok(lines.some((line) => line.includes(internals.RELEASES_PAGE)))
  })
})
