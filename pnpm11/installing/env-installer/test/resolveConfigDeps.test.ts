import path from 'node:path'

import { expect, test } from '@jest/globals'
import { resolveConfigDeps } from '@pnpm/installing.env-installer'
import { readEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { readYamlFileSync } from 'read-yaml-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('configuration dependency is resolved', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  await resolveConfigDeps(['@pnpm.e2e/foo@100.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    store: storeController,
    storeDir,
  })

  // Workspace manifest should have a clean specifier (no integrity)
  const workspaceManifest = readYamlFileSync<{ configDependencies: Record<string, string> }>('pnpm-workspace.yaml')
  expect(workspaceManifest.configDependencies).toStrictEqual({
    '@pnpm.e2e/foo': '100.0.0',
  })

  // Env lockfile should contain the resolved dependency with integrity
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toStrictEqual({
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0'),
    },
  })
  expect(envLockfile!.snapshots['@pnpm.e2e/foo@100.0.0']).toStrictEqual({})
})

test('one level of optionalDependencies is recorded in the env lockfile with platform fields', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  await resolveConfigDeps(['@pnpm.e2e/support-different-architectures@1.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    store: storeController,
    storeDir,
  })

  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()

  const parentKey = '@pnpm.e2e/support-different-architectures@1.0.0'
  expect(envLockfile!.snapshots[parentKey]).toStrictEqual({
    optionalDependencies: {
      '@pnpm.e2e/only-darwin-arm64': '1.0.0',
      '@pnpm.e2e/only-darwin-x64': '1.0.0',
      '@pnpm.e2e/only-linux-arm64-glibc': '1.0.0',
      '@pnpm.e2e/only-linux-arm64-musl': '1.0.0',
      '@pnpm.e2e/only-linux-x64-glibc': '1.0.0',
      '@pnpm.e2e/only-linux-x64-musl': '1.0.0',
      '@pnpm.e2e/only-win32-arm64': '1.0.0',
      '@pnpm.e2e/only-win32-x64': '1.0.0',
    },
  })

  // Each optional subdep is in `packages` with its os/cpu fields preserved for
  // install-time platform filtering, and gets an `optional: true` snapshot
  // to match how optional packages are recorded elsewhere in the lockfile.
  expect(envLockfile!.packages['@pnpm.e2e/only-darwin-arm64@1.0.0']).toStrictEqual({
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/only-darwin-arm64', '1.0.0'),
    },
    os: ['darwin'],
    cpu: ['arm64'],
  })
  expect(envLockfile!.snapshots['@pnpm.e2e/only-darwin-arm64@1.0.0']).toStrictEqual({ optional: true })
  // libc is preserved alongside os/cpu for musl/glibc variants.
  expect(envLockfile!.packages['@pnpm.e2e/only-linux-x64-musl@1.0.0']).toStrictEqual({
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/only-linux-x64-musl', '1.0.0'),
    },
    os: ['linux'],
    cpu: ['x64'],
    libc: ['musl'],
  })

  // The parent config dep itself is still registered as the only top-level config dep.
  expect(envLockfile!.importers['.'].configDependencies).toStrictEqual({
    '@pnpm.e2e/support-different-architectures': {
      specifier: '1.0.0',
      version: '1.0.0',
    },
  })
})

test('config dep with no optionalDependencies keeps an empty snapshot', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  await resolveConfigDeps(['@pnpm.e2e/foo@100.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    store: storeController,
    storeDir,
  })

  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile!.snapshots['@pnpm.e2e/foo@100.0.0']).toStrictEqual({})
})

test('rejects an optionalDependency declared with a non-exact version', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  // @pnpm.e2e/foobar declares `@pnpm.e2e/bar: "^100.0.0"` — a range, not an exact version.
  await expect(resolveConfigDeps(['@pnpm.e2e/foobar@100.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    store: storeController,
    storeDir,
  })).rejects.toThrow(/only exact versions are supported/)
})

test('orphan optional subdeps from a previous resolution are pruned', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  // Simulate a prior resolution that left optional subdeps for a now-removed
  // version of a config dependency. The stale `foo@99.0.0` and its optional
  // subdep `bar@1.0.0` are not referenced from any current configDependency.
  await writeEnvLockfile(process.cwd(), {
    lockfileVersion: '9.0',
    importers: {
      '.': { configDependencies: {} },
    },
    packages: {
      '@pnpm.e2e/foo@99.0.0': { resolution: { integrity: 'sha512-stale==' } },
      '@pnpm.e2e/bar@1.0.0': { resolution: { integrity: 'sha512-stale==' } },
    },
    snapshots: {
      '@pnpm.e2e/foo@99.0.0': { optionalDependencies: { '@pnpm.e2e/bar': '1.0.0' } },
      '@pnpm.e2e/bar@1.0.0': { optional: true },
    },
  })

  await resolveConfigDeps(['@pnpm.e2e/foo@100.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    store: storeController,
    storeDir,
  })

  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile!.packages['@pnpm.e2e/foo@99.0.0']).toBeUndefined()
  expect(envLockfile!.packages['@pnpm.e2e/bar@1.0.0']).toBeUndefined()
  expect(envLockfile!.snapshots['@pnpm.e2e/foo@99.0.0']).toBeUndefined()
  expect(envLockfile!.snapshots['@pnpm.e2e/bar@1.0.0']).toBeUndefined()
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toBeDefined()
})

test('fails with frozenLockfile', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  await expect(resolveConfigDeps(['@pnpm.e2e/foo@100.0.0'], {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    store: storeController,
    storeDir,
    frozenLockfile: true,
  })).rejects.toThrow('Cannot resolve configDependencies with "frozen-lockfile"')
})
