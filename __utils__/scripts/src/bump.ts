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
  const repoRoot = path.resolve(import.meta.dirname, '../../..')
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
