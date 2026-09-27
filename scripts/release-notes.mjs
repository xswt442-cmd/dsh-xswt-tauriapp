import fs from 'node:fs'
import { matchRelease } from './changelog.mjs'

const version = process.argv[2]
// A prerelease tail is allowed here because `release.yml` cuts `vX.Y.Z-rc.N` tags
// and marks such a release as one. Rejecting the version this script is *given*
// broke the notes step inside `set -euo pipefail` for exactly the tags the
// workflow supports.
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version ?? '')) {
  console.error('usage: node scripts/release-notes.mjs X.Y.Z')
  process.exit(2)
}

let active = false
const lines = []
for (const line of fs.readFileSync('CHANGELOG.md', 'utf8').split(/\r?\n/)) {
  if (line.startsWith('## ')) {
    if (active) break
    active = matchRelease(line)?.version === version
    continue
  }
  if (active) lines.push(line)
}

const body = lines.join('\n').trim()
if (!body) {
  // Loud rather than fatal. The workflow's own rule is that a missing section
  // costs the notes their text, never the release, so this stays a warning — but
  // a release page reading `Release 0.1.0` with nothing under it looks like a
  // deliberate choice unless somebody says otherwise in the log.
  console.error(`::warning::CHANGELOG.md has no '## ${version}' section; the notes get no body`)
}
process.stdout.write(body || `Release ${version}`)
