import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { git, temporaryRepo } from './git-fixture.mjs'
import { isPnprPackage, parseOptions, selectPackages, smokeStandIns, trackedBase, unselectedDependents, workspaceWideChanges } from './test-affected.mjs'

const script = fileURLToPath(new URL('./test-affected.mjs', import.meta.url))

function repoWithOneCommit (context) {
  const repo = temporaryRepo(context, 'pnpm-test-affected-')
  git(repo, 'commit', '--allow-empty', '-m', 'root', '--no-verify')
  return repo
}

const manifests = [
  { name: 'pnpm-cli', dir: 'pnpm/crates/cli' },
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
  assert.deepEqual(selectPackages(['AGENTS.md', 'pnpm11/pnpm/src/main.ts'], manifests), [])
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
  assert.deepEqual(
    selectPackages(['pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch'], manifests),
    ['pnpm-cli'])
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
  assert.deepEqual(workspaceWideChanges(['pnpm/crates/testing-utils/src/bin.rs']), ['pnpm/crates/testing-utils/src/bin.rs'])
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
