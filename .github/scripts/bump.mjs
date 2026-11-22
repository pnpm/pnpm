// Applies the pending release plan, then runs the meta-updater to mirror the
// bumped npm wrapper versions into the Rust sources the release builds from.
//
// `--release <product>` (repeatable) restricts the run to a subset of the
// releasable products. With no `--release` flag every pending intent is
// consumed, so a bare `pnpm bump` cuts a full release.

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

// The workspace packages each product releases, keyed by the product's
// `--release` token. `pnpm` is the Rust CLI: published to npm as `pnpm`, named
// `pacquet` in-repo, with its versioning.fixed `@pnpm/napi` sibling.
export const PRODUCT_PACKAGES = {
  pnpm: ['pacquet', '@pnpm/napi'],
  pnpr: ['@pnpm/pnpr'],
  'get-pnpm': ['get-pnpm'],
}

// The Rust products' npm wrappers are `"private": true` in-repo so that a
// workspace-wide `pnpm publish` never ships them; their generate-packages
// scripts drop the flag when they are really published. `pnpm version -r`
// commits a private project's changelog to its CHANGELOG.md, but release.yml
// reads the parked section in `.changeset/changelogs/`, so
// parkPublishedPrivateChangelogs moves it there.
const PUBLISHED_PRIVATE_PROJECT_DIRS = [
  'pnpm/npm/pnpm',
  'pnpm/npm/napi',
  'pnpr/npm/pnpr',
]

function main () {
  const repoRoot = findRepoRoot(path.dirname(fileURLToPath(import.meta.url)))
  const filterArgs = releaseFilterArgs(parseSelectedProducts(process.argv.slice(2)))
  // The release PR branch is dirty here (refreshed trust roots, synthesized
  // changesets), so skip the clean-tree check.
  execFileSync('pnpm', ['version', '-r', '--no-git-checks', ...filterArgs], { cwd: repoRoot, stdio: 'inherit' })
  parkPublishedPrivateChangelogs(repoRoot, PUBLISHED_PRIVATE_PROJECT_DIRS)
  execFileSync('pnpm', ['update-manifests'], { cwd: repoRoot, stdio: 'inherit' })
}

/** The products named by `--release`. Fails closed on any other argument. */
export function parseSelectedProducts (argv) {
  const selected = new Set()
  // `pnpm run bump -- --release ...` forwards the `--` separator to the script.
  const start = argv[0] === '--' ? 1 : 0
  for (let argIndex = start; argIndex < argv.length; argIndex++) {
    if (argv[argIndex] !== '--release') {
      throw new Error(`Unexpected bump argument: ${String(argv[argIndex])}. Only "--release <product>" is supported.`)
    }
    const product = argv[++argIndex]
    if (!Object.hasOwn(PRODUCT_PACKAGES, product ?? '')) {
      throw new Error(`Unknown --release product: ${String(product)}. Expected one of ${Object.keys(PRODUCT_PACKAGES).join(', ')}.`)
    }
    selected.add(product)
  }
  return selected
}

/** `--filter` arguments for `pnpm version -r`. An empty selection releases everything. */
export function releaseFilterArgs (selected) {
  return [...selected].flatMap((product) => PRODUCT_PACKAGES[product]).map((pkg) => `--filter=${pkg}`)
}

/**
 * Moves the CHANGELOG.md that `pnpm version -r` wrote for each project in
 * `projectDirs` to `.changeset/changelogs/<name>@<version>.md`. A project the
 * run did not release has no CHANGELOG.md and is skipped.
 */
export function parkPublishedPrivateChangelogs (repoRoot, projectDirs) {
  for (const projectDir of projectDirs) {
    const changelogPath = path.join(repoRoot, projectDir, 'CHANGELOG.md')
    if (!fs.existsSync(changelogPath)) continue
    const manifest = JSON.parse(fs.readFileSync(path.join(repoRoot, projectDir, 'package.json'), 'utf8'))
    const changelog = fs.readFileSync(changelogPath, 'utf8')
    const versionHeadings = changelog.split('\n').filter((line) => line.startsWith('## '))
    const sectionStart = changelog.indexOf(`\n## ${manifest.version}\n`) + 1
    if (sectionStart === 0 || versionHeadings.length !== 1) {
      throw new Error(`${changelogPath} must hold exactly one section, for ${manifest.name}@${manifest.version}`)
    }
    const parkedPath = path.join(repoRoot, '.changeset', 'changelogs', `${manifest.name}@${manifest.version}`.replaceAll('/', '!') + '.md')
    fs.mkdirSync(path.dirname(parkedPath), { recursive: true })
    fs.writeFileSync(parkedPath, changelog.slice(sectionStart))
    fs.rmSync(changelogPath)
  }
}

export function findRepoRoot (startDir) {
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

if (process.argv[1] && import.meta.url === pathToFileURL(fs.realpathSync(process.argv[1])).href) {
  main()
}
