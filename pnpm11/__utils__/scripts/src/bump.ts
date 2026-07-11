// Wrapper around `changeset version` that prevents cherry-picked changesets
// from being applied twice when a release branch is merged back into main.
// Maintains a per-branch ledger at .changeset-released/<branch>.txt of
// consumed changeset ids; before running `changeset version` it hides any
// changeset whose id is already in the union of those files. See
// .changeset-released/README.md for the full explanation.
//
// The ledger lives outside `.changeset/` because `@changesets/read` treats
// every directory inside `.changeset/` as a legacy v1 changeset and tries to
// read `changes.md` from it.
//
// After `changeset version` it also keeps the Rust products' prerelease
// version lines going (`continuePrereleases`) and copies the bumped wrapper
// versions into the Rust sources the release builds from
// (`syncRustVersions`).

import { execSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import url from 'node:url'

import glob from 'fast-glob'

export interface HiddenFile {
  id: string
  from: string
  to: string
}

export function branchToFilename (branch: string): string {
  return `${branch.replace(/\//g, '-')}.txt`
}

// Release commits land via a PR whose branch is named `release-pr/<target>`,
// where `<target>` is the branch the release is for (`main`, `release/11.1`, …).
// The ledger must be keyed by that target, not the ephemeral PR branch, so that
// every release for `main` accumulates in `main.txt` rather than scattering into
// a new file per PR. A branch without the prefix (e.g. a direct release on
// `main`) is its own target.
export const RELEASE_PR_PREFIX = 'release-pr/'

export function releaseBranchToTarget (branch: string): string {
  if (!branch.startsWith(RELEASE_PR_PREFIX)) return branch
  const target = branch.slice(RELEASE_PR_PREFIX.length)
  if (target === '') {
    throw new Error(
      `Branch "${branch}" has no target after "${RELEASE_PR_PREFIX}"; expected e.g. "${RELEASE_PR_PREFIX}main".`
    )
  }
  return target
}

export function readReleased (releasedDir: string): Set<string> {
  const ids = new Set<string>()
  if (!fs.existsSync(releasedDir)) return ids
  for (const file of fs.readdirSync(releasedDir)) {
    if (!file.endsWith('.txt')) continue
    const content = fs.readFileSync(path.join(releasedDir, file), 'utf8')
    for (const line of content.split('\n')) {
      const id = line.trim()
      if (id !== '' && !id.startsWith('#')) ids.add(id)
    }
  }
  return ids
}

export function appendReleased (
  releasedDir: string,
  branch: string,
  ids: readonly string[]
): void {
  if (ids.length === 0) return
  fs.mkdirSync(releasedDir, { recursive: true })
  const file = path.join(releasedDir, branchToFilename(branch))
  const merged = new Set<string>()
  if (fs.existsSync(file)) {
    for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
      const id = line.trim()
      if (id !== '' && !id.startsWith('#')) merged.add(id)
    }
  }
  for (const id of ids) merged.add(id)
  const sorted = [...merged].sort()
  fs.writeFileSync(file, `${sorted.join('\n')}\n`)
}

export function listChangesetIds (changesetDir: string): string[] {
  const files = glob.sync('*.md', { cwd: changesetDir, ignore: ['README.md'] })
  return files.map(f => path.basename(f, '.md')).sort()
}

export function hideReleased (changesetDir: string, released: Set<string>): HiddenFile[] {
  const hidden: HiddenFile[] = []
  try {
    for (const id of listChangesetIds(changesetDir)) {
      if (!released.has(id)) continue
      const from = path.join(changesetDir, `${id}.md`)
      const to = path.join(changesetDir, `${id}.md.released`)
      fs.renameSync(from, to)
      hidden.push({ id, from, to })
    }
  } catch (err) {
    restoreHidden(hidden)
    throw err
  }
  return hidden
}

export function restoreHidden (hidden: readonly HiddenFile[]): void {
  for (const h of hidden) {
    if (fs.existsSync(h.to)) fs.renameSync(h.to, h.from)
  }
}

export function deleteHidden (hidden: readonly HiddenFile[]): void {
  for (const h of hidden) {
    if (fs.existsSync(h.to)) fs.unlinkSync(h.to)
  }
}

// The npm wrapper manifests of the Rust products. These are the packages that
// may sit on a prerelease line (see continuePrereleases) and whose versions
// are mirrored into Rust sources (see syncRustVersions).
export const RUST_CLI_WRAPPER_MANIFEST = 'pnpm/npm/pnpm/package.json'
export const NAPI_WRAPPER_MANIFEST = 'pnpm/npm/napi/package.json'
export const PNPR_WRAPPER_MANIFEST = 'pnpr/npm/pnpr/package.json'

export const PRERELEASE_CONTINUATION_MANIFESTS = [
  RUST_CLI_WRAPPER_MANIFEST,
  NAPI_WRAPPER_MANIFEST,
  PNPR_WRAPPER_MANIFEST,
]

export interface PrereleaseLine {
  base: string
  tag: string
  n: number
}

// Matches `<X.Y.Z>-<tag>.<N>` prerelease versions with a named tag (`alpha`,
// `beta`, `rc.`..). Deliberately does not match all-numeric suffixes like the
// retired date-based `0.0.0-<date>` scheme — those have no line to continue.
const PRERELEASE_VERSION_RE = /^(\d+\.\d+\.\d+)-([A-Z][0-9A-Z-]*)\.(\d+)$/i

export function snapshotPrereleases (
  repoRoot: string,
  manifestPaths: readonly string[]
): Map<string, PrereleaseLine> {
  const snapshot = new Map<string, PrereleaseLine>()
  for (const manifestPath of manifestPaths) {
    const version = readManifestVersion(path.join(repoRoot, manifestPath))
    const match = PRERELEASE_VERSION_RE.exec(version)
    if (match === null) continue
    snapshot.set(manifestPath, { base: match[1], tag: match[2], n: Number(match[3]) })
  }
  return snapshot
}

// Outside changesets' pre mode, `changeset version` turns `X.Y.Z-tag.N` plus
// any bump type into plain `X.Y.Z`. Pre mode is global to the workspace, so it
// cannot be used while the TypeScript packages release stable versions from
// the same branch. Instead, a package that sat on `X.Y.Z-tag.N` before the
// bump and landed on exactly `X.Y.Z` is rewritten to `X.Y.Z-tag.N+1`
// (manifest and changelog heading). Leaving the prerelease line — releasing
// the stable `X.Y.Z` — is a deliberate manual version edit in a release PR.
export function continuePrereleases (
  repoRoot: string,
  snapshot: ReadonlyMap<string, PrereleaseLine>
): void {
  for (const [manifestPath, line] of snapshot) {
    const absManifestPath = path.join(repoRoot, manifestPath)
    const manifest = JSON.parse(fs.readFileSync(absManifestPath, 'utf8'))
    if (manifest.version !== line.base) continue
    const continued = `${line.base}-${line.tag}.${line.n + 1}`
    console.log(`Continuing prerelease line of ${manifest.name}: ${continued}`)
    manifest.version = continued
    fs.writeFileSync(absManifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    continueChangelogHeading(path.join(path.dirname(absManifestPath), 'CHANGELOG.md'), line.base, continued)
  }
}

// The Rust sources embed the versions their release builds report; the npm
// wrapper manifests bumped by changesets are the source of truth. The release
// workflow verifies the copies match, so a missed sync fails the release
// instead of shipping a binary with a stale --version.
export function syncRustVersions (repoRoot: string): void {
  const pacquetVersion = readManifestVersion(path.join(repoRoot, RUST_CLI_WRAPPER_MANIFEST))
  replaceInFile(
    path.join(repoRoot, 'pnpm/crates/config/src/defaults.rs'),
    /(pub const PNPM_VERSION: &str = )"[^"]*"/,
    `$1"${pacquetVersion}"`
  )
  const pnprVersion = readManifestVersion(path.join(repoRoot, PNPR_WRAPPER_MANIFEST))
  replaceInFile(
    path.join(repoRoot, 'pnpr/crates/pnpr/Cargo.toml'),
    /^(version\s*=\s*)"[^"]*"/m,
    `$1"${pnprVersion}"`
  )
  replaceInFile(
    path.join(repoRoot, 'Cargo.lock'),
    /(\[\[package\]\]\nname = "pnpr"\nversion = )"[^"]*"/,
    `$1"${pnprVersion}"`
  )
}

function readManifestVersion (absManifestPath: string): string {
  const manifest = JSON.parse(fs.readFileSync(absManifestPath, 'utf8'))
  if (typeof manifest.version !== 'string') {
    throw new Error(`No version field in ${absManifestPath}`)
  }
  return manifest.version
}

function continueChangelogHeading (changelogPath: string, base: string, continued: string): void {
  if (!fs.existsSync(changelogPath)) return
  const lines = fs.readFileSync(changelogPath, 'utf8').split('\n')
  const headingIndex = lines.indexOf(`## ${base}`)
  if (headingIndex === -1) return
  lines[headingIndex] = `## ${continued}`
  fs.writeFileSync(changelogPath, lines.join('\n'))
}

function replaceInFile (file: string, pattern: RegExp, replacement: string): void {
  const content = fs.readFileSync(file, 'utf8')
  if (!pattern.test(content)) {
    throw new Error(`Pattern ${pattern} not found in ${file}`)
  }
  fs.writeFileSync(file, content.replace(pattern, replacement))
}

export function findRepoRoot (startDir: string): string {
  let dir = startDir
  while (!fs.existsSync(path.join(dir, '.changeset'))) {
    const parent = path.dirname(dir)
    if (parent === dir) {
      throw new Error(`No .changeset directory found above ${startDir}`)
    }
    dir = parent
  }
  return dir
}

function detectReleaseBranch (cwd: string): string {
  const override = process.env.RELEASE_BRANCH?.trim()
  if (override !== undefined && override !== '') return releaseBranchToTarget(override)
  const out = execSync('git rev-parse --abbrev-ref HEAD', { cwd, encoding: 'utf8' }).trim()
  if (out === 'HEAD') {
    throw new Error(
      'Detached HEAD; set RELEASE_BRANCH to override the current branch name.'
    )
  }
  return releaseBranchToTarget(out)
}

function main (): void {
  const repoRoot = findRepoRoot(import.meta.dirname)
  const changesetDir = path.join(repoRoot, '.changeset')
  const releasedDir = path.join(repoRoot, '.changeset-released')
  const branch = detectReleaseBranch(repoRoot)

  console.log(`Branch: ${branch}`)
  const released = readReleased(releasedDir)
  console.log(`Already-released changeset IDs: ${released.size}`)

  const hidden = hideReleased(changesetDir, released)
  if (hidden.length > 0) {
    console.log(
      `Hiding ${hidden.length} stale changeset(s) already released elsewhere: ${hidden.map(h => h.id).join(', ')}`
    )
  }

  const prereleases = snapshotPrereleases(repoRoot, PRERELEASE_CONTINUATION_MANIFESTS)

  const before = listChangesetIds(changesetDir)
  let success = false
  try {
    execSync('changeset version', { cwd: repoRoot, stdio: 'inherit' })
    success = true
  } finally {
    if (!success) restoreHidden(hidden)
  }

  try {
    const after = new Set(listChangesetIds(changesetDir))
    const newlyConsumed = before.filter(id => !after.has(id))
    if (newlyConsumed.length > 0) {
      console.log(`Recording newly-released: ${newlyConsumed.join(', ')}`)
      appendReleased(releasedDir, branch, newlyConsumed)
    }
  } finally {
    // Stale (cherry-picked, already released elsewhere) changesets get dropped
    // from the working tree — the released-list already prevents re-application.
    deleteHidden(hidden)
  }

  continuePrereleases(repoRoot, prereleases)
  syncRustVersions(repoRoot)
}

function isDirectInvocation (): boolean {
  if (process.argv[1] === undefined) return false
  try {
    return import.meta.url === url.pathToFileURL(fs.realpathSync(process.argv[1])).href
  } catch {
    return false
  }
}

if (isDirectInvocation()) {
  main()
}
