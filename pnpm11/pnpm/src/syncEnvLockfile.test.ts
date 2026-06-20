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
const resolvePackageManagerIntegrities = jest.fn<(version: string, opts: { envLockfile?: EnvLockfile, registries?: unknown, rootDir: string, save?: boolean }) => Promise<EnvLockfile>>(
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

test('writes packageManagerDependencies when no env lockfile exists yet (#11674)', async () => {
  const dir = tempDir()
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))
  expect(resolvePackageManagerIntegrities).toHaveBeenCalledTimes(1)
  const updated = await readEnvLockfile(dir)
  expect(updated).not.toBeNull()
  expect(updated!.importers['.'].packageManagerDependencies?.['pnpm']).toEqual({
    specifier: packageManager.version,
    version: packageManager.version,
  })
})

test('writes packageManagerDependencies when env lockfile exists but lacks pnpm entry (#11674)', async () => {
  const dir = tempDir()
  writeEnvLockfileWithoutPmDeps(dir)
  await syncEnvLockfile(baseConfig, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))
  expect(resolvePackageManagerIntegrities).toHaveBeenCalledTimes(1)
  const updated = await readEnvLockfile(dir)
  expect(updated!.importers['.'].packageManagerDependencies?.['pnpm']).toEqual({
    specifier: packageManager.version,
    version: packageManager.version,
  })
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

test('uses trusted package-manager registries instead of project registries', async () => {
  const dir = tempDir()
  const projectRegistries = {
    '@pnpm': 'https://project-pnpm.example.com/',
    default: 'https://project.example.com/',
  }
  const packageManagerRegistries = {
    '@pnpm': 'https://trusted-pnpm.example.com/',
    default: 'https://trusted.example.com/',
  }
  const packageManagerNetworkConfig = {
    configByUri: {
      '//trusted.example.com/': { '@': { authToken: 'trusted-token' } },
    },
    httpProxy: 'http://trusted-http-proxy.example.com:8080',
    httpsProxy: 'http://trusted-https-proxy.example.com:8080',
    noProxy: 'trusted.internal',
    strictSsl: true,
  }

  await syncEnvLockfile({
    configByUri: {
      '//project.example.com/': { '@': { authToken: 'project-token' } },
    },
    httpProxy: 'http://project-http-proxy.example.com:8080',
    httpsProxy: 'http://project-https-proxy.example.com:8080',
    noProxy: 'project.internal',
    packageManagerRegistries,
    packageManagerNetworkConfig,
    registries: projectRegistries,
    strictSsl: false,
  } as unknown as Config, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))

  expect(createStoreController).toHaveBeenCalledWith(expect.objectContaining({
    configByUri: packageManagerNetworkConfig.configByUri,
    httpProxy: packageManagerNetworkConfig.httpProxy,
    httpsProxy: packageManagerNetworkConfig.httpsProxy,
    noProxy: packageManagerNetworkConfig.noProxy,
    registries: packageManagerRegistries,
    strictSsl: packageManagerNetworkConfig.strictSsl,
  }))
  expect(resolvePackageManagerIntegrities).toHaveBeenCalledWith(packageManager.version, expect.objectContaining({
    registries: packageManagerRegistries,
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalledWith(packageManager.version, expect.objectContaining({
    registries: projectRegistries,
  }))
})

test('defaults package-manager registries to npmjs instead of project registries', async () => {
  const dir = tempDir()
  const projectRegistries = {
    '@pnpm': 'https://project-pnpm.example.com/',
    default: 'https://project.example.com/',
  }

  await syncEnvLockfile({
    configByUri: {
      '//project.example.com/': { '@': { authToken: 'project-token' } },
    },
    httpProxy: 'http://project-http-proxy.example.com:8080',
    httpsProxy: 'http://project-https-proxy.example.com:8080',
    noProxy: 'project.internal',
    registries: projectRegistries,
    strictSsl: false,
  } as unknown as Config, makeContext(dir, {
    wantedPackageManager: { name: 'pnpm', version: packageManager.version, fromDevEngines: true },
  }))

  expect(createStoreController).toHaveBeenCalledWith(expect.objectContaining({
    configByUri: {},
    httpProxy: undefined,
    httpsProxy: undefined,
    noProxy: undefined,
    registries: { default: 'https://registry.npmjs.org/' },
    strictSsl: undefined,
  }))
  expect(resolvePackageManagerIntegrities).toHaveBeenCalledWith(packageManager.version, expect.objectContaining({
    registries: { default: 'https://registry.npmjs.org/' },
  }))
  expect(resolvePackageManagerIntegrities).not.toHaveBeenCalledWith(packageManager.version, expect.objectContaining({
    registries: projectRegistries,
  }))
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
