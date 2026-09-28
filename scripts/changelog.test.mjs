import assert from 'node:assert/strict'
import test from 'node:test'
import { matchRelease, matchUnreleased, UNRELEASED } from './changelog.mjs'

/**
 * The headings `check-docs.mjs` and `release-notes.mjs` share.
 *
 * These cases exist because the two scripts used to spell the same heading
 * differently, and a heading neither recognised left both changelogs parsing to
 * an empty list — which the bilingual comparison then passed as agreement. So the
 * interesting assertions here are the ones that must NOT match: a shape the
 * checker silently stops seeing is the shape that breaks the guard.
 */

test('a released section is read the way docs/RELEASING.md asks for it', () => {
  assert.deepEqual(matchRelease('## 0.0.13 - 2026-09-24'), { version: '0.0.13', date: '2026-09-24' })
  // A prerelease tail is part of the version: `release.yml` cuts `vX.Y.Z-rc.N`
  // tags and marks such a release as one.
  assert.deepEqual(matchRelease('## 0.0.14-rc.1 - 2026-10-01'), {
    version: '0.0.14-rc.1',
    date: '2026-10-01',
  })
  // The date is what a section is stamped with at release time; a heading written
  // without one is still a release.
  assert.deepEqual(matchRelease('## 0.0.1'), { version: '0.0.1', date: '' })
})

test('the unreleased section is its own thing, not a version', () => {
  assert.equal(matchUnreleased('## Unreleased'), true)
  // Both scripts must agree it names no release: `release-notes.mjs` takes its
  // body by version, and an entry landing here is not a release body.
  assert.equal(matchRelease('## Unreleased'), null)
  assert.equal(matchUnreleased('## 0.0.13 - 2026-09-24'), false)
  assert.equal(UNRELEASED, 'Unreleased')
})

test('a heading the checker cannot read is never mistaken for a release', () => {
  // Every one of these used to be matched by one of the two scripts and skipped
  // by the other. Now neither matches, and `check-docs.mjs` fails on a changelog
  // with nothing readable in it rather than comparing two empty lists.
  for (const line of [
    '## [0.0.13] - 2026-09-24',
    '## v0.0.13 - 2026-09-24',
    '## 0.0.13 — 2026-09-24',
    '## 0.0.13 (2026-09-24)',
    '## 0.0',
    '## Unreleased-ish',
    '## License',
    '### 新增',
    '# Changelog',
  ]) {
    assert.equal(matchRelease(line), null, `${line} is not a release heading`)
  }
})

test('a trailing space or an extra heading level does not pass', () => {
  // The anchor is the whole line, so a suffix nobody meant to add is a heading the
  // checker does not see — which is the loud failure above, not a quiet one.
  assert.equal(matchRelease('## 0.0.13 - 2026-09-24 - draft'), null)
  assert.equal(matchRelease('#### 0.0.13 - 2026-09-24'), null)
  assert.equal(matchRelease('##  0.0.13  -  2026-09-24').version, '0.0.13')
})
