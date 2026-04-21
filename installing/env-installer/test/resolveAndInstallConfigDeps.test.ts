import path from 'node:path'

import { expect, test } from '@jest/globals'
import { resolveAndInstallConfigDeps } from '@pnpm/installing.env-installer'
import { createEnvLockfile, readEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { loadJsonFileSync } from 'load-json-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

function createOpts () {
  const { storeController, storeDir } = createTempStore()
  return {
    registries: { default: registry },
    rootDir: process.cwd(),
    cacheDir: path.resolve('cache'),
    userConfig: {},
    store: storeController,
    storeDir,
  }
}

test('resolves and installs config dep when no env lockfile exists', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Simulate a config dep manually added to pnpm-workspace.yaml (clean specifier, no lockfile)
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)

  // Package should be installed
  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')

  // Env lockfile should be created with resolved info
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
})

test('resolves newly added config dep when env lockfile already has other deps', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create env lockfile with one dep
  const existingLockfile = createEnvLockfile()
  existingLockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  existingLockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  existingLockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), existingLockfile)

  // Now install with an additional dep
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
    '@pnpm.e2e/bar': '100.0.0',
  }, opts)

  // Both packages should be installed
  const fooManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(fooManifest.name).toBe('@pnpm.e2e/foo')
  expect(fooManifest.version).toBe('100.0.0')

  const barManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/bar/package.json')
  expect(barManifest.name).toBe('@pnpm.e2e/bar')
  expect(barManifest.version).toBe('100.0.0')

  // Env lockfile should have both deps
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toBeDefined()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/bar']).toBeDefined()
})

test('skips resolution when all deps are already in env lockfile', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create complete env lockfile
  const lockfile = createEnvLockfile()
  lockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  lockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  lockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), lockfile)

  // Install should work without network (using lockfile data)
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')
})

test('re-resolves and reinstalls when config dep version changes in pnpm-workspace.yaml', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create env lockfile with foo@100.0.0
  const lockfile = createEnvLockfile()
  lockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  lockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  lockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), lockfile)

  // Install first with the old version
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, opts)

  // Now simulate user changing the version in pnpm-workspace.yaml
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.1.0',
  }, opts)

  // The new version should be installed
  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.1.0')

  // Env lockfile should be updated with the new version
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.1.0',
    version: '100.1.0',
  })
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.1.0']).toStrictEqual({
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.1.0'),
    },
  })
  // Old version should be cleaned up from the lockfile
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toBeUndefined()
})

test('handles old format config deps via migration path', async () => {
  prepareEmpty()
  const opts = createOpts()

  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
  }, opts)

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')
})

test('handles mixed old-format and new-format config deps together', async () => {
  prepareEmpty()
  const opts = createOpts()

  // One dep in old inline-integrity format, another as a clean specifier
  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
    '@pnpm.e2e/bar': '100.0.0',
  }, opts)

  // Both packages should be installed
  const fooManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(fooManifest.name).toBe('@pnpm.e2e/foo')
  expect(fooManifest.version).toBe('100.0.0')

  const barManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/bar/package.json')
  expect(barManifest.name).toBe('@pnpm.e2e/bar')
  expect(barManifest.version).toBe('100.0.0')

  // Env lockfile should have both deps
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toBeDefined()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/bar']).toBeDefined()
  expect(envLockfile!.packages['@pnpm.e2e/foo@100.0.0']).toBeDefined()
  expect(envLockfile!.packages['@pnpm.e2e/bar@100.0.0']).toBeDefined()
})

test('fails with frozenLockfile when old-format deps need migration', async () => {
  prepareEmpty()
  const opts = createOpts()

  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  await expect(resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
  }, { ...opts, frozenLockfile: true })).rejects.toThrow('Cannot update configDependencies with "frozen-lockfile"')
})

test('fails with frozenLockfile when new-format deps need resolution', async () => {
  prepareEmpty()
  const opts = createOpts()

  await expect(resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, { ...opts, frozenLockfile: true })).rejects.toThrow('Cannot update configDependencies with "frozen-lockfile"')
})

test('succeeds with frozenLockfile when env lockfile is up-to-date', async () => {
  prepareEmpty()
  const opts = createOpts()

  // Pre-create complete env lockfile
  const lockfile = createEnvLockfile()
  lockfile.importers['.'].configDependencies['@pnpm.e2e/foo'] = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  lockfile.packages['@pnpm.e2e/foo@100.0.0'] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  }
  lockfile.snapshots['@pnpm.e2e/foo@100.0.0'] = {}
  await writeEnvLockfile(process.cwd(), lockfile)

  await resolveAndInstallConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, { ...opts, frozenLockfile: true })

  const manifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
  expect(manifest.name).toBe('@pnpm.e2e/foo')
  expect(manifest.version).toBe('100.0.0')
})
