import assert from 'node:assert/strict'
import { test } from 'node:test'
import { isPnprPackage, selectPackages, workspaceWideChanges } from './test-affected.mjs'

const manifests = [
  { name: 'pnpm-cli', dir: 'pnpm/crates/cli' },
  { name: 'pnpm-lockfile', dir: 'pnpm/crates/lockfile' },
  { name: 'pnpm-registry-mock', dir: 'pnpm/tasks/registry-mock' },
  { name: 'pnpr', dir: 'pnpr/crates/pnpr' },
  { name: 'pnpr-storage', dir: 'pnpr/crates/storage' },
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
