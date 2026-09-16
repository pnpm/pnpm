import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { isPnprPackage, parseOptions, selectPackages, unselectedDependents, workspaceWideChanges } from './test-affected.mjs'

const script = fileURLToPath(new URL('./test-affected.mjs', import.meta.url))

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

test('counts the dependents whose tests the selection leaves out', () => {
  const packages = [
    { name: 'fs', dependencies: [] },
    { name: 'lockfile', dependencies: ['fs'] },
    { name: 'cli', dependencies: ['lockfile'] },
    { name: 'unrelated', dependencies: [] },
  ]
  assert.deepEqual([...unselectedDependents(['fs'], packages)], [['fs', 2]])
  assert.deepEqual([...unselectedDependents(['fs', 'lockfile', 'cli'], packages)], [])
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
})

test('fails when the base revision cannot be resolved', (context) => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-affected-'))
  context.after(() => fs.rmSync(repo, { recursive: true, force: true }))
  for (const args of [['init'], ['commit', '--allow-empty', '-m', 'root', '--no-verify']]) {
    spawnSync('git', args, { cwd: repo, env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' } })
  }
  const result = spawnSync('node', [script, '--base', 'no-such-branch'], { cwd: repo, encoding: 'utf8' })
  assert.equal(result.status, 1)
  assert.match(result.stderr, /no merge base between 'no-such-branch'/)
})
