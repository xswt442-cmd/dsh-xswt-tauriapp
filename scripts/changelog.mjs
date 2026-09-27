/**
 * The two heading forms the repository's changelogs use.
 *
 * `## Unreleased` carries the entries made since the last release, and
 * `## X.Y.Z - YYYY-MM-DD` is a released one. Both scripts that read a changelog
 * go through this file, and that sharing is the point.
 *
 * `check-docs.mjs` used to anchor on the plain form and `release-notes.mjs` on
 * either, so a heading written a different way — brackets, an em dash before the
 * date, a `v` prefix — made *both* changelogs parse to no releases at all. The
 * bilingual check then compared two empty lists, passed, and the guard that
 * exists to catch a one-sided entry said nothing while the release notes
 * silently degraded to a bare `Release X.Y.Z`. `check-docs.mjs` now fails on a
 * changelog it cannot read a version out of, which is what makes the shared
 * heading the single source rather than a second opinion.
 */
const RELEASE_HEADING =
  /^##\s+(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)(?:\s+-\s+(\d{4}-\d{2}-\d{2}))?\s*$/
const UNRELEASED_HEADING = /^##\s+Unreleased\s*$/

/**
 * The version `changelogShape` reports for the unreleased section, so callers
 * can tell it from a released one without spelling the word out.
 */
export const UNRELEASED = 'Unreleased'

/**
 * `{ version, date }` when `line` opens a released section, else `null`.
 *
 * A prerelease tail is part of the version because `release.yml` cuts `vX.Y.Z-rc.N`
 * tags and marks such a release as one. `version` is the text as the changelog
 * spells it, with no `v`, so it compares straight against what the manifests
 * declare.
 */
export function matchRelease(line) {
  const found = line.match(RELEASE_HEADING)
  if (!found) return null
  return { version: found[1], date: found[2] ?? '' }
}

/** Whether `line` opens the section that is not a release yet. */
export function matchUnreleased(line) {
  return UNRELEASED_HEADING.test(line)
}
