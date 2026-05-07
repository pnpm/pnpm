import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { readEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { tempDir } from '@pnpm/prepare'

// Simulate what the real resolvePackageManagerIntegrities does that this test
// cares about: record the resolved pnpm version under
// packageManagerDependencies and persist the lockfile to disk.
const resolvePackageManagerIntegrities = jest.fn<(version: string, opts: { envLockfile?: EnvLockfile, rootDir: string, save?: boolean }) => Promise<EnvLockfile>>(
  async (version, opts) => {
    const lockfile = opts.envLockfile ?? ({ lockfileVersion: '9.0', importers: { '.': { configDependencies: {} } }, packages: {}, snapshots: {} } as EnvLockfile)
    lockfile.importers['.'].packageManagerDependencies = {
      pnpm: { specifier: version, version },
      '@pnpm/exe': { specifier: version, version },
    }
    if (opts.save) await writeEnvLockfile(opts.rootDir, lockfile)
    return lockfile
  }
)
const createStoreController = jest.fn<(opts: object) => Promise<{ ctrl: { close: () => Promise<void> }, dir: string }>>(async () => ({
  ctrl: { close: jest.fn<() => Promise<void>>(async () => {}) },
  dir: '/store',
}))

jest.unstable_mockModule('@pnpm/installing.env-installer', () => ({
  resolvePackageManagerIntegrities,
}))

jest.unstable_mockModule('@pnpm/store.connection-manager', () => ({
  createStoreController,
}))

const { syncEnvLockfile } = await import('./syncEnvLockfile.js')

beforeEach(() => {
  resolvePackageManagerIntegrities.mockClear()
  createStoreController.mockClear()
})

function makeContext (rootDir: string, overrides: Partial<ConfigContext> = {}): ConfigContext {
  return {
    rootProjectManifestDir: rootDir,
    wantedPackageManager: undefined,
    ...overrides,
  } as ConfigContext
}

const baseConfig = { registries: { default: 'https://registry.npmjs.org/' } } as unknown as Config

test('no-op when wantedPackageManager is undefined', async () => {
  const dir = tempDir()
  await syncEnvLockfile(baseConfig, makeContext(dir))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
})

test('no-op when wantedPackageManager is not pnpm', async () => {
  const dir = tempDir()
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'yarn', version: '4.0.0', fromDevEngines: true },
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
})

test('no-op when shouldPersistLockfile is false (legacy packageManager < v12)', async () => {
  const dir = tempDir()
  writeStaleEnvLockfile(dir, '9.0.0')
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: '11.0.0' },
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
})

test('no-op when running pnpm does not satisfy wanted range', async () => {
  const dir = tempDir()
  writeStaleEnvLockfile(dir, '9.0.0')
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: '0.0.1', fromDevEngines: true },
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
})

test('no-op when no env lockfile exists', async () => {
  const dir = tempDir()
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
})

test('no-op when lockfile has no packageManagerDependencies for pnpm', async () => {
  const dir = tempDir()
  writeEnvLockfileWithoutPmDeps(dir)
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
})

test('no-op when lockfile already records a satisfying version', async () => {
  const dir = tempDir()
  writeStaleEnvLockfile(dir, packageManager.version)
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalled()
})

test('updates the lockfile when locked version no longer satisfies wanted version', async () => {
  const dir = tempDir()
  writeStaleEnvLockfile(dir, '9.0.0')
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))
  const updated = await readEnvLockfile(dir)
  expect(updated).not.toBeNull()
  expect(updated!.importers['.'].packageManagerDependencies?.['pnpm']).toEqual({
    specifier: packageManager.version,
    version: packageManager.version,
  })
})

function writeStaleEnvLockfile (dir: string, pnpmVersion: string): void {
  // readEnvLockfile expects a multi-document YAML file beginning with `---\n`,
  // where the env lockfile is the first document.
  const envYaml = `lockfileVersion: '9.0'
importers:
  '.':
    configDependencies: {}
    packageManagerDependencies:
      pnpm:
        specifier: ${pnpmVersion}
        version: ${pnpmVersion}
packages: {}
snapshots: {}
`
  fs.writeFileSync(path.join(dir, 'pnpm-lock.yaml'), `---\n${envYaml}\n---\n`)
}

function writeEnvLockfileWithoutPmDeps (dir: string): void {
  const envYaml = `lockfileVersion: '9.0'
importers:
  '.':
    configDependencies: {}
packages: {}
snapshots: {}
`
  fs.writeFileSync(path.join(dir, 'pnpm-lock.yaml'), `---\n${envYaml}\n---\n`)
}
