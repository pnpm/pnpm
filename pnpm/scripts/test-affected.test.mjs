import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { git, temporaryRepo } from './git-fixture.mjs'
import { e2eAreas, e2eFilter, executionOptions, isPnprPackage, mappedE2eModules, parseOptions, selectPackages, smokeStandIns, trackedBase, unselectedDependents, workspaceWideChanges } from './test-affected.mjs'

const script = fileURLToPath(new URL('./test-affected.mjs', import.meta.url))
const repo = path.resolve(fileURLToPath(new URL('../..', import.meta.url)))

function repoWithOneCommit (context) {
  const repo = temporaryRepo(context, 'pnpm-test-affected-')
  git(repo, 'commit', '--allow-empty', '-m', 'root', '--no-verify')
  return repo
}

const manifests = [
  { name: 'pnpm-cli', dir: 'pnpm/crates/cli' },
  { name: 'pnpm-config', dir: 'pnpm/crates/config' },
  { name: 'pnpm-lockfile', dir: 'pnpm/crates/lockfile' },
  { name: 'pnpm-registry-mock', dir: 'pnpm/tasks/registry-mock' },
  { name: 'pnpr', dir: 'pnpr/crates/pnpr' },
  { name: 'pnpr-storage', dir: 'pnpr/crates/storage' },
  { name: 'pnpm-deps-restorer', dir: 'pnpm/crates/deps-restorer' },
]

test('selects the crate that owns each changed file', () => {
  assert.deepEqual(
    selectPackages(['pnpm/crates/lockfile/src/lib.rs', 'pnpm/crates/cli/tests/suite/add.rs'], manifests),
    ['pnpm-cli', 'pnpm-lockfile'])
})

test('ignores files outside the workspace', () => {
  assert.deepEqual(selectPackages(['AGENTS.md', 'pnpm/docs/cli/install.md'], manifests), [])
})

test('selects every pnpr crate when one of them changes', () => {
  assert.deepEqual(
    selectPackages(['pnpr/crates/storage/src/lib.rs'], manifests),
    ['pnpm-registry-mock', 'pnpr', 'pnpr-storage'])
})

test('prefers the closest crate for a nested manifest', () => {
  const nested = [{ name: 'outer', dir: 'pnpm/crates/cli' }, { name: 'inner', dir: 'pnpm/crates/cli/fixtures/pkg' }]
  assert.deepEqual(selectPackages(['pnpm/crates/cli/fixtures/pkg/src/lib.rs'], nested), ['inner'])
})

test('selects the crates that read a fixture stored outside them', () => {
  assert.deepEqual(selectPackages(['fixtures/gvs-link-hash-parity.json'], manifests), ['pnpm-deps-restorer'])
  assert.deepEqual(selectPackages(['fixtures/pin-decision.json'], manifests), ['pnpm-cli', 'pnpm-config'])
  for (const oracleInput of ['pnpm/test-oracles/package.json', '.config/pnpm/catalog.yaml', 'pnpm-lock.yaml']) {
    assert.deepEqual(selectPackages([oracleInput], manifests), ['pnpm-cli', 'pnpm-config', 'pnpm-fs-packlist'])
  }
})

test('refuses to scope a change to the mocked registry packages', () => {
  assert.deepEqual(workspaceWideChanges(['pnpr/.fixtures/packages/foo/package.json']),
    ['pnpr/.fixtures/packages/foo/package.json'])
})

test('reports the changes that affect every crate', () => {
  assert.deepEqual(
    workspaceWideChanges(['Cargo.lock', '.cargo/config.toml', 'pnpm/crates/lockfile/src/lib.rs']),
    ['Cargo.lock', '.cargo/config.toml'])
})

test('a crate manifest is not a workspace-wide change', () => {
  assert.deepEqual(workspaceWideChanges(['pnpm/crates/lockfile/Cargo.toml']), [])
})

test('identifies the crates that must be selected together', () => {
  assert.ok(isPnprPackage('pnpr-storage'))
  assert.ok(isPnprPackage('pnpm-registry-mock'))
  assert.ok(!isPnprPackage('pnpm-lockfile'))
})

test('refuses to scope a change to the shared test harness', () => {
  assert.deepEqual(
    workspaceWideChanges(['pnpm/crates/testing-utils/src/lib.rs', 'pnpm/crates/testing-utils-registry/src/registry.rs']),
    ['pnpm/crates/testing-utils/src/lib.rs', 'pnpm/crates/testing-utils-registry/src/registry.rs'])
})

const graph = [
  { name: 'pnpm-fs', dependencies: [] },
  { name: 'pnpm-lockfile', dependencies: ['pnpm-fs'] },
  { name: 'pnpm-cli', dependencies: ['pnpm-lockfile'] },
  { name: 'pnpm-micro-benchmark', dependencies: [] },
]

test('names the dependents whose tests the selection leaves out', () => {
  assert.deepEqual([...unselectedDependents(['pnpm-fs'], graph)], [['pnpm-fs', ['pnpm-cli', 'pnpm-lockfile']]])
  assert.deepEqual([...unselectedDependents(['pnpm-fs', 'pnpm-lockfile', 'pnpm-cli'], graph)], [])
})

test('stands in for unselected dependents with smoke tests', () => {
  const selected = ['pnpm-fs']
  assert.deepEqual(smokeStandIns(selected, unselectedDependents(selected, graph)), ['pnpm-cli'])
})

test('skips smoke tests when the CLI is selected in full', () => {
  const selected = ['pnpm-cli', 'pnpm-fs']
  assert.deepEqual(smokeStandIns(selected, unselectedDependents(selected, graph)), [])
})

test('skips smoke tests when nothing depends on what changed', () => {
  const selected = ['pnpm-micro-benchmark']
  assert.deepEqual(smokeStandIns(selected, unselectedDependents(selected, graph)), [])
})

test('maps a core crate to the CLI modules that observe its effects', () => {
  assert.deepEqual(e2eAreas(['pnpm-lockfile']), [
    'add', 'ci_frozen_lockfile', 'current_lockfile', 'dedupe', 'import',
    'install', 'lockfile_dir', 'lockfile_format_v13', 'remove', 'update',
  ])
})

test('unions and deduplicates the modules for several changed crates', () => {
  const areas = e2eAreas(['pnpm-fs', 'pnpm-lockfile'])
  assert.ok(areas.includes('cas_linker'))
  assert.ok(areas.includes('update'))
  assert.equal(areas.filter(area => area === 'install').length, 1)
})

test('leaves crates without a module map to their own tests and smoke', () => {
  assert.deepEqual(e2eAreas(['pnpm-micro-benchmark']), [])
})

test('every mapped module is declared in the CLI suite', () => {
  const source = fs.readFileSync(path.join(repo, 'pnpm/crates/cli/tests/suite/main.rs'), 'utf8')
  const modules = new Set([...source.matchAll(/^mod\s+([\w]+);$/gm)].map(([, name]) => name))
  for (const area of mappedE2eModules()) {
    assert.ok(modules.has(area), `E2E_AREAS names ${area}, but suite/main.rs does not declare mod ${area};`)
  }
})

test('builds an anchored nextest filter from module names', () => {
  assert.equal(e2eFilter(['add', 'install']), 'test(/^add::/) + test(/^install::/)')
})

test('forwards execution options without forwarding a run selection', () => {
  const args = ['--no-run', '-p', 'pnpm-cli', '-E', 'test(add::)', '--profile', 'smoke', '--no-capture']
  assert.deepEqual(executionOptions(args), ['--no-run', '--no-capture'])
  assert.deepEqual(executionOptions(['-pfoo', '-Etest(add::)', '--package=bar', '--filter=test(a)', '--profile=smoke', '--no-run']), ['--no-run'])
})

test('reads --no-smoke', () => {
  assert.equal(parseOptions([]).values.smoke, true)
  assert.equal(parseOptions(['--no-smoke']).values.smoke, false)
})

test('forwards every argument nextest understands, in order', () => {
  const { values, rest } = parseOptions(['--base', 'HEAD~1', '-p', 'pnpm-cli', '-E', 'test(catalog::)'])
  assert.equal(values.base, 'HEAD~1')
  assert.deepEqual(rest, ['-p', 'pnpm-cli', '-E', 'test(catalog::)'])
})

test('reads its own options in either spelling', () => {
  assert.equal(parseOptions(['--base=HEAD']).values.base, 'HEAD')
  assert.equal(parseOptions(['--print']).values.print, true)
  assert.equal(parseOptions([]).values.base, 'main')
})

test('forwards everything after a bare double dash', () => {
  const { values, rest } = parseOptions(['--print', '--', '--no-capture', '--base', 'not-ours'])
  assert.equal(values.print, true)
  assert.deepEqual(rest, ['--no-capture', '--base', 'not-ours'])
})

test('rejects --base without a revision', () => {
  assert.throws(() => parseOptions(['--base']), /--base needs a revision/)
  assert.throws(() => parseOptions(['--base', '--help']), /--base needs a revision, not '--help'/)
  assert.throws(() => parseOptions(['--base=']), /--base needs a revision, not ''/)
})

test('diffs a branch name against its remote-tracking ref', (context) => {
  const repo = repoWithOneCommit(context)
  git(repo, 'branch', 'release/1.0')
  assert.equal(trackedBase(repo, 'main'), 'main')
  assert.equal(trackedBase(repo, 'release/1.0'), 'release/1.0')
  for (const ref of ['refs/remotes/origin/main', 'refs/remotes/origin/release/1.0']) {
    git(repo, 'update-ref', ref, 'HEAD')
  }
  assert.equal(trackedBase(repo, 'main'), 'origin/main')
  assert.equal(trackedBase(repo, 'release/1.0'), 'origin/release/1.0')
})

test('diffs against a revision that is not a branch name as given', (context) => {
  const repo = repoWithOneCommit(context)
  for (const ref of ['refs/tags/v1', 'refs/remotes/origin/HEAD', 'refs/remotes/origin/main', 'refs/remotes/origin/v1']) {
    git(repo, 'update-ref', ref, 'HEAD')
  }
  assert.equal(trackedBase(repo, 'HEAD'), 'HEAD')
  assert.equal(trackedBase(repo, 'main~1'), 'main~1')
  assert.equal(trackedBase(repo, 'v1'), 'v1')
  assert.equal(trackedBase(repo, 'origin/main'), 'origin/main')
})

test('fails when the base revision cannot be resolved', (context) => {
  const repo = repoWithOneCommit(context)
  const result = spawnSync('node', [script, '--base', 'no-such-branch'], { cwd: repo, encoding: 'utf8' })
  assert.equal(result.status, 1)
  assert.match(result.stderr, /no merge base between 'no-such-branch'/)
})
