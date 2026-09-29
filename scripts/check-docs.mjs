import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import { matchRelease, matchUnreleased, UNRELEASED } from './changelog.mjs'

const read = (file) => fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n')

// The documentation pairs this repository ships. Every path the checks below read
// comes from this list, so moving a document into a directory is an edit here.
const pairs = [
  { name: 'README', zh: 'README.md', en: 'README.en.md' },
  { name: 'CHANGELOG', zh: 'docs/CHANGELOG.md', en: 'docs/CHANGELOG.en.md' },
]
const pairOf = (name) => pairs.find((pair) => pair.name === name)

function markdownShape(file) {
  let fenced = false
  const headings = []
  const fences = []

  for (const line of read(file).split('\n')) {
    const fence = line.match(/^\s*```\s*(\S*)/)
    if (fence) {
      if (!fenced) fences.push(fence[1])
      fenced = !fenced
      continue
    }
    if (fenced) continue
    const heading = line.match(/^(#{1,3})\s+/)
    if (heading) headings.push(heading[1].length)
  }

  if (fenced) throw new Error(`${file}: unclosed code fence`)
  return { headings, fences }
}

function changelogShape(file) {
  const releases = []
  let release
  let section

  for (const line of read(file).split('\n')) {
    const released = matchRelease(line)
    const opened = released ?? (matchUnreleased(line) ? { version: UNRELEASED, date: '' } : null)
    if (opened) {
      release = { version: opened.version, date: opened.date, sections: [] }
      releases.push(release)
      section = undefined
      continue
    }
    if (!release) continue

    const heading = line.match(/^###\s+(.+?)\s*$/)
    if (heading) {
      section = { title: heading[1], items: 0 }
      release.sections.push(section)
      continue
    }
    if (section && /^\s*-\s+/.test(line)) section.items += 1
  }

  return releases
}

const category = new Map([
  ['新增', 'added'], ['Added', 'added'],
  ['修复', 'fixed'], ['Fixed', 'fixed'],
  ['变更', 'changed'], ['Changed', 'changed'],
  ['移除', 'removed'], ['Removed', 'removed'],
  ['安全', 'security'], ['Security', 'security'],
  ['性能', 'performance'], ['Performance', 'performance'],
  ['兼容性', 'compatibility'], ['Compatibility', 'compatibility'],
  ['维护', 'maintenance'], ['Maintenance', 'maintenance'],
])

function assertEqual(left, right, message) {
  if (JSON.stringify(left) !== JSON.stringify(right)) {
    throw new Error(`${message}\nleft:  ${JSON.stringify(left)}\nright: ${JSON.stringify(right)}`)
  }
}

const readme = pairOf('README')
assertEqual(markdownShape(readme.zh), markdownShape(readme.en), 'README structure differs between languages')

const changelog = pairOf('CHANGELOG')
const changelogs = [changelog.zh, changelog.en].map((file) => {
  const releases = changelogShape(file)
  // Two shapes that a drifted heading spelling produces, both of which have to be
  // errors rather than a comparison of two empty lists: nothing readable as a
  // release, and an `## Unreleased` that is missing, doubled, or not first.
  // Entries are written under that section and the release renames it, so this is
  // where the bilingual guard has to bite — before a release exists, not at the
  // tag.
  const unreleased = releases.filter(({ version }) => version === UNRELEASED)
  if (unreleased.length !== 1 || releases[0]?.version !== UNRELEASED) {
    throw new Error(`${file}: '## Unreleased' must appear exactly once, as the first section`)
  }
  const versioned = releases.filter(({ version }) => version !== UNRELEASED)
  if (versioned.length === 0) {
    throw new Error(`${file}: no released section in the form '## X.Y.Z - YYYY-MM-DD'`)
  }
  return { file, releases, newest: versioned[0].version }
})

const shape = ({ releases }) => releases.map((release) => ({
  version: release.version,
  date: release.date,
  sections: release.sections.map(({ title, items }) => ({
    title: category.get(title) ?? title.toLowerCase(),
    items,
  })),
}))

assertEqual(shape(changelogs[0]), shape(changelogs[1]), 'CHANGELOG structure differs between languages')

/**
 * The package version a manifest declares.
 *
 * Read the way `release.yml` reads it: the first `version = "..."` line, which
 * is the `[package]` one. A dependency line spells its version inside braces.
 */
const manifestVersion = (file) => {
  const found = read(file).match(/^version = "(.*)"$/m)
  if (!found) throw new Error(`${file}: no package version`)
  return found[1]
}

// The five version fields move together, and a drift must surface here rather
// than at tag time, where `release.yml` compares the same five.
const fields = [
  ['package.json', JSON.parse(read('package.json')).version],
  ['src-tauri/tauri.conf.json', JSON.parse(read('src-tauri/tauri.conf.json')).version],
  ['src-tauri/Cargo.toml', manifestVersion('src-tauri/Cargo.toml')],
  ['crates/dsh-core/Cargo.toml', manifestVersion('crates/dsh-core/Cargo.toml')],
  ['plugins/dsh-desktop-app/package.json', JSON.parse(read('plugins/dsh-desktop-app/package.json')).version],
]

const [[firstFile, first], ...others] = fields
for (const [file, version] of others) {
  if (version !== first) {
    throw new Error(`version fields disagree: ${firstFile} says ${first}, ${file} says ${version}`)
  }
}
console.log(`the five version fields agree on ${first}`)

// ...and they agree with the changelog's newest *released* section, which is the
// one the workflow appends verbatim as the release body. A bump that forgets the
// log passes every check above — they only compare the five fields with each
// other — and the release page then reads `Release X.Y.Z` with nothing under it.
const newest = changelogs[0].newest
if (newest !== first) {
  throw new Error(
    `${changelogs[0].file}: newest released section is ${newest} but the version fields say ${first} ` +
      '(released sections are newest first)',
  )
}

/** Git's all-zero revision, which GitHub reports as `before` for a new branch. */
const NULL_REVISION = /^0+$/

const baseIndex = process.argv.indexOf('--base')
if (baseIndex !== -1) {
  const base = process.argv[baseIndex + 1]
  if (!base) throw new Error('--base requires a Git revision')

  if (NULL_REVISION.test(base)) {
    // A branch's first push reports `before` as the null revision, so there is
    // no previous state to compare against. Failing here would fail every new
    // branch; the paired-edit rule has nothing to say about a first import.
    console.log(`no base revision (${base}); skipping the paired-edit check`)
  } else {
    const changed = new Set(execFileSync('git', ['diff', '--name-only', base, 'HEAD'], { encoding: 'utf8' })
      .split(/\r?\n/)
      .filter(Boolean))

    for (const pair of pairs) {
      if (changed.has(pair.zh) !== changed.has(pair.en)) {
        throw new Error(`${pair.zh} and ${pair.en} must change together`)
      }
    }
  }
}

console.log('bilingual docs are structurally aligned')
