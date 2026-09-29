/**
 * The two heading forms the repository's changelogs use.
 *
 * `## Unreleased` carries the entries made since the last release, and
 * `## X.Y.Z - YYYY-MM-DD` is a released one. Both scripts that read a changelog
 * go through this file, and that sharing is the point.
 *
 * A heading written any other way — brackets, an em dash before the date, a `v`
 * prefix — matches neither form here, so a changelog reads as holding no releases
 * at all. `check-docs.mjs` fails on that rather than comparing two empty lists as
 * agreement, which is what makes this file the single source of the heading
 * instead of two readers that can disagree.
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
