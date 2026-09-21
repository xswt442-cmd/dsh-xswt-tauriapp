import { execFileSync } from 'node:child_process'
import fs from 'node:fs'

const read = (file) => fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n')

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
    const version = line.match(/^##\s+(\d+\.\d+\.\d+)(?:\s+-\s+(\d{4}-\d{2}-\d{2}))?\s*$/)
    if (version) {
      release = { version: version[1], date: version[2] ?? '', sections: [] }
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

assertEqual(markdownShape('README.md'), markdownShape('README.en.md'), 'README structure differs between languages')

const normalizeLog = (file) => changelogShape(file).map((release) => ({
  version: release.version,
  date: release.date,
  sections: release.sections.map(({ title, items }) => ({
    title: category.get(title) ?? title.toLowerCase(),
    items,
  })),
}))
assertEqual(normalizeLog('CHANGELOG.md'), normalizeLog('CHANGELOG.en.md'), 'CHANGELOG structure differs between languages')

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

// The five version fields move together, and until now the only thing that
// checked them was `release.yml` — at tag time, long after a drift could be
// introduced on `dev`.
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

    for (const [primary, translation] of [
      ['README.md', 'README.en.md'],
      ['CHANGELOG.md', 'CHANGELOG.en.md'],
    ]) {
      if (changed.has(primary) !== changed.has(translation)) {
        throw new Error(`${primary} and ${translation} must change together`)
      }
    }
  }
}

console.log('bilingual docs are structurally aligned')
