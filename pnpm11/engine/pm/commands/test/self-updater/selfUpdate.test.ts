import { createHash } from 'node:crypto'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals'
import { linkBins } from '@pnpm/bins.linker'
import { STORE_VERSION } from '@pnpm/constants'
import type { PnpmError } from '@pnpm/error'
import { prepare as prepareWithPkg, tempDir } from '@pnpm/prepare'
import { prependDirsToPath } from '@pnpm/shell.path'
import { getRegisteredProjects } from '@pnpm/store.controller'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import spawn from 'cross-spawn'
import { familySync } from 'detect-libc'

const require = createRequire(import.meta.dirname)
const pnpmTarballPath = require.resolve('@pnpm/tgz-fixtures/tgz/pnpm-9.1.0.tgz')

const actualModule = await import('@pnpm/cli.meta')
const mockPackageManager = {
  name: 'pnpm',
  version: '9.0.0',
}
jest.unstable_mockModule('@pnpm/cli.meta', () => {
  return {
    ...actualModule,
    packageManager: mockPackageManager,
  }
})
const { selfUpdate, assertPnpmRuns, assertReleaseIsInstallable, installPnpm, linkExePlatformBinary, exePlatformPkgDirName, exePlatformPkgDirNameNext, pnpmPackageNameToInstall } = await import('@pnpm/engine.pm.commands')

beforeEach(async () => {
  mockPackageManager.version = '9.0.0'
  await setupMockAgent()
  getMockAgent().enableNetConnect()
})

afterEach(async () => {
  await teardownMockAgent()
})

function prepare (manifest: object = {}) {
  const dir = tempDir(false)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest), 'utf8')
  return prepareOptions(dir)
}

function prepareOptions (dir: string) {
  return {
    argv: {
      original: [],
    },
    cliOptions: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    bail: true,
    globalPkgDir: path.join(dir, 'global', 'v11'),
    pnpmHomeDir: dir,
    preferWorkspacePackages: true,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    sort: false,
    rootProjectManifestDir: dir,
    bin: path.join(dir, 'bin'),
    workspaceConcurrency: 1,
    extraEnv: {},
    pnpmfile: '',
    configByUri: {},
    cacheDir: path.join(dir, '.cache'),
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    dir,
    // The fixture pnpm installed here is not signed with npm's real keys, so
    // skip the engine identity signature check (empty trusted-keys = skip).
    trustedKeys: [],
  }
}

function createMetadata (
  latest: string,
  registry: string,
  otherVersions: string[] = [],
  time: Record<string, string> = {}
) {
  const versions = [...otherVersions, latest]
  return {
    name: 'pnpm',
    'dist-tags': { latest },
    versions: Object.fromEntries(versions.map((version) => [
      version,
      {
        name: 'pnpm',
        version,
        dist: {
          shasum: '217063ce3fcbf44f3051666f38b810f1ddefee4a',
          tarball: `${registry}pnpm/-/pnpm-${version}.tgz`,
          fileCount: 880,
          integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
        },
      },
    ])),
    time,
  }
}

function createExeMetadata (version: string, registry: string) {
  return {
    name: '@pnpm/exe',
    'dist-tags': { latest: version },
    versions: {
      [version]: {
        name: '@pnpm/exe',
        version,
        dist: {
          shasum: 'abcdef1234567890',
          tarball: `${registry}@pnpm/exe/-/exe-${version}.tgz`,
          integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
        },
      },
    },
  }
}

/**
 * Mock @pnpm/exe metadata for tests that call resolvePackageManagerIntegrities.
 * This prevents install() from making real HTTP requests for @pnpm/exe.
 */
function mockExeMetadata (registry: string, version: string) {
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: '/@pnpm%2Fexe', method: 'GET' }) // cspell:disable-line
    .reply(200, createExeMetadata(version, registry))
}

/**
 * Mock all registry requests needed for a full self-update flow.
 * This includes: initial resolution, resolvePackageManagerIntegrities, and handleGlobalAdd.
 */
function mockRegistryForUpdate (registry: string, version: string, metadata: object) {
  // Use persist for metadata since multiple components request it
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata).persist()
  mockExeMetadata(registry, version)
  const tgzData = fs.readFileSync(pnpmTarballPath)
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: `/pnpm/-/pnpm-${version}.tgz`, method: 'GET' })
    .reply(200, tgzData)
}

function seedGlobalPnpm (opts: ReturnType<typeof prepareOptions>, version: string): string {
  const installDir = path.join(opts.globalPkgDir, `pnpm-${version}`)
  const pkgDir = path.join(installDir, 'node_modules', 'pnpm')
  fs.mkdirSync(pkgDir, { recursive: true })
  fs.writeFileSync(path.join(installDir, 'package.json'), JSON.stringify({ dependencies: { pnpm: version } }), 'utf8')
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'pnpm', version, bin: { pnpm: 'bin.js' } }), 'utf8')
  fs.writeFileSync(path.join(pkgDir, 'bin.js'), `#!/usr/bin/env node
console.log('${version}')`, 'utf8')
  fs.symlinkSync(installDir, path.join(opts.globalPkgDir, `hash-${version}`))
  return installDir
}

test('self-update', async () => {
  const opts = prepare()
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  await selfUpdate.handler(opts, [])

  // Verify the package was installed in the global dir.
  // The globalDir contains both the real install dir (a directory) and a
  // hash symlink pointing to it. Use lstatSync to pick the real dir.
  const globalDir = path.join(opts.pnpmHomeDir, 'global', 'v11')
  const entries = fs.readdirSync(globalDir)
  const installDirName = entries.find((e) => fs.lstatSync(path.join(globalDir, e)).isDirectory())
  expect(installDirName).toBeDefined()
  const installDir = path.join(globalDir, installDirName!)
  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(installDir, 'node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')

  // Verify the install dir was registered in the store's project registry.
  // Without this, `pnpm store prune` would remove the install's packages
  // from the global virtual store.
  const storeDir = path.join(opts.pnpmHomeDir, 'store', STORE_VERSION)
  const registeredProjects = await getRegisteredProjects(storeDir)
  expect(registeredProjects).toContain(installDir)

  const pnpmEnv = prependDirsToPath([path.join(opts.pnpmHomeDir, 'bin')])
  const { status, stdout } = spawn.sync('pnpm', ['-v'], {
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  expect(status).toBe(0)
  expect(stdout.toString().trim()).toBe('9.1.0')
})

test('self-update refreshes legacy v10 bootstrap shim at pnpmHomeDir', async () => {
  // pnpm v10 setup added pnpmHomeDir (not pnpmHomeDir/bin) to PATH and wrote
  // a `pnpm` bootstrap shim there. After upgrading to v11, that shim still
  // points into the old `.tools/<version>` install, so PATH continues to
  // resolve to the pre-update pnpm. Self-update on v11 must refresh the
  // legacy shim so the upgrade actually takes effect for users still on the
  // v10 PATH layout. See pnpm/pnpm#11464.
  const opts = prepare()
  // Simulate a leftover v10 bootstrap shim. Content is irrelevant — the
  // detector only cares about file presence, and linkBins will overwrite it.
  fs.writeFileSync(path.join(opts.pnpmHomeDir, 'pnpm'), '#!/bin/sh\necho stale\n', { mode: 0o755 })
  if (process.platform === 'win32') {
    fs.writeFileSync(path.join(opts.pnpmHomeDir, 'pnpm.cmd'), '@echo stale\n')
  }
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  await selfUpdate.handler(opts, [])

  // Invoking pnpm via pnpmHomeDir (the v10 PATH layout, NOT pnpmHomeDir/bin)
  // must now resolve to the freshly installed version.
  const pnpmEnv = prependDirsToPath([opts.pnpmHomeDir])
  const { status, stdout } = spawn.sync('pnpm', ['-v'], {
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  expect(status).toBe(0)
  expect(stdout.toString().trim()).toBe('9.1.0')
})

test('self-update does not write shims to pnpmHomeDir on a clean v11 layout', async () => {
  // Mirror image of the previous test: when there is no v10-style shim at
  // pnpmHomeDir, self-update must NOT start writing bins there. Otherwise we
  // would clutter pnpmHomeDir on every fresh-v11 self-update.
  const opts = prepare()
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  await selfUpdate.handler(opts, [])

  expect(fs.existsSync(path.join(opts.pnpmHomeDir, 'pnpm'))).toBe(false)
  expect(fs.existsSync(path.join(opts.pnpmHomeDir, 'pnpm.cmd'))).toBe(false)
})

test('self-update by exact version', async () => {
  const opts = prepare()
  const metadata = createMetadata('9.2.0', opts.registries.default, ['9.1.0'])
  const registry = opts.registries.default.replace(/\/$/, '')
  getMockAgent().get(registry)
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata).persist()
  mockExeMetadata(opts.registries.default, '9.1.0')
  const tgzData = fs.readFileSync(pnpmTarballPath)
  getMockAgent().get(registry)
    .intercept({ path: '/pnpm/-/pnpm-9.1.0.tgz', method: 'GET' })
    .reply(200, tgzData)

  await selfUpdate.handler(opts, ['9.1.0'])

  // Verify the package was installed in the global dir
  const globalDir = path.join(opts.pnpmHomeDir, 'global', 'v11')
  const entries = fs.readdirSync(globalDir)
  const installDirName = entries.find((e) => fs.statSync(path.join(globalDir, e)).isDirectory())
  expect(installDirName).toBeDefined()
  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(globalDir, installDirName!, 'node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')

  const pnpmEnv = prependDirsToPath([path.join(opts.pnpmHomeDir, 'bin')])
  const { status, stdout } = spawn.sync('pnpm', ['-v'], {
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  expect(status).toBe(0)
  expect(stdout.toString().trim()).toBe('9.1.0')
})

test('self-update does nothing when pnpm is up to date', async () => {
  const opts = prepare()
  seedGlobalPnpm(opts, '9.0.0')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default))

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe('The currently active pnpm v9.0.0 is already "latest" and doesn\'t need an update')
})

test('self-update installs the active pnpm version when it is missing from the global dir', async () => {
  mockPackageManager.version = '9.1.0'
  const opts = prepare()
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  const output = await selfUpdate.handler(opts, ['9.1.0'])

  expect(output).toBe('Successfully updated pnpm to v9.1.0')
  const globalEntries = fs.readdirSync(opts.globalPkgDir)
  const installDirName = globalEntries.find((e) => fs.lstatSync(path.join(opts.globalPkgDir, e)).isDirectory())
  expect(installDirName).toBeDefined()
  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(opts.globalPkgDir, installDirName!, 'node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')

  const pnpmEnv = prependDirsToPath([path.join(opts.pnpmHomeDir, 'bin')])
  const { status, stdout } = spawn.sync('pnpm', ['-v'], {
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  expect(status).toBe(0)
  expect(stdout.toString().trim()).toBe('9.1.0')
})

test('self-update respects minimumReleaseAge for implicit latest resolution', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0',
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  const now = Date.now()
  const metadata = createMetadata('9.1.0', opts.registries.default, ['9.0.0'], {
    '9.0.0': new Date(now - 48 * 60 * 60 * 1000).toISOString(),
    '9.1.0': new Date(now - 8 * 60 * 60 * 1000).toISOString(),
  })
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata)

  const output = await selfUpdate.handler({
    ...opts,
    minimumReleaseAge: 24 * 60,
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@9.0.0')
})

test('self-update rejects a trust downgrade under trustPolicy=no-downgrade', async () => {
  const opts = prepare()
  const registry = opts.registries.default
  const now = Date.now()
  // The earlier 9.0.5 was published with strong trust evidence (trusted
  // publisher + provenance); the later 9.1.0 has none — a trust downgrade
  // the no-downgrade policy must refuse to switch to.
  const metadata = {
    name: 'pnpm',
    'dist-tags': { latest: '9.1.0' },
    time: {
      '9.0.5': new Date(now - 48 * 60 * 60 * 1000).toISOString(),
      '9.1.0': new Date(now - 8 * 60 * 60 * 1000).toISOString(),
    },
    versions: {
      '9.0.5': {
        name: 'pnpm',
        version: '9.0.5',
        _npmUser: {
          name: 'pnpm-bot',
          trustedPublisher: { id: 'github', oidcConfigId: 'release' },
        },
        dist: {
          shasum: '217063ce3fcbf44f3051666f38b810f1ddefee4a',
          tarball: `${registry}pnpm/-/pnpm-9.0.5.tgz`,
          integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
          attestations: { provenance: { predicateType: 'https://slsa.dev/provenance/v1' } },
        },
      },
      '9.1.0': {
        name: 'pnpm',
        version: '9.1.0',
        dist: {
          shasum: '217063ce3fcbf44f3051666f38b810f1ddefee4a',
          tarball: `${registry}pnpm/-/pnpm-9.1.0.tgz`,
          integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
        },
      },
    },
  }
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata).persist()

  await expect(
    selfUpdate.handler({ ...opts, trustPolicy: 'no-downgrade' }, [])
  ).rejects.toThrow(/High-risk trust downgrade/)
})

test('self-update does not write packageManagerDependencies when package manager onFail is ignore', async () => {
  const opts = prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '^9.0.0',
      },
    },
  })
  const lockfilePath = path.join(opts.dir, 'pnpm-lock.yaml')
  fs.writeFileSync(lockfilePath, [
    '---',
    "lockfileVersion: '9.0'",
    '',
    'importers:',
    '',
    '  .:',
    '    configDependencies: {}',
    '',
    'packages: {}',
    'snapshots: {}',
    '---',
    '',
  ].join('\n'), 'utf8')
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '^9.0.0',
      fromDevEngines: true,
      onFail: 'ignore',
    },
  }, [])

  expect(fs.readFileSync(lockfilePath, 'utf8')).not.toContain('packageManagerDependencies')
})

test('global self-update respects minimumReleaseAge: skips immature latest, no-op when older mature matches active', async () => {
  // Reproduces #11655: a globally-installed pnpm (no project pin / no
  // wantedPackageManager) must not jump to a "latest" version younger than
  // minimumReleaseAge. Active pnpm is mocked as 9.0.0 at the top of this
  // file. The registry's `latest` (9.1.0) is 8h old — immature — so the
  // resolver should fall back to 9.0.0, which equals the active version and is
  // already installed globally, producing a no-op rather than reinstalling.
  const opts = prepare()
  seedGlobalPnpm(opts, '9.0.0')
  const globalEntriesBefore = fs.readdirSync(opts.globalPkgDir).sort()
  const now = Date.now()
  const metadata = createMetadata('9.1.0', opts.registries.default, ['9.0.0'], {
    '9.0.0': new Date(now - 48 * 60 * 60 * 1000).toISOString(),
    '9.1.0': new Date(now - 8 * 60 * 60 * 1000).toISOString(),
  })
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata)

  const output = await selfUpdate.handler({
    ...opts,
    minimumReleaseAge: 24 * 60,
  }, [])

  expect(output).toBe('The currently active pnpm v9.0.0 is already "latest" and doesn\'t need an update')
  expect(fs.readdirSync(opts.globalPkgDir).sort()).toStrictEqual(globalEntriesBefore)
})

test('self-update respects minimumReleaseAgeExclude for implicit latest resolution', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0',
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  const now = Date.now()
  const metadata = createMetadata('9.1.0', opts.registries.default, ['9.0.0'], {
    '9.0.0': new Date(now - 48 * 60 * 60 * 1000).toISOString(),
    '9.1.0': new Date(now - 8 * 60 * 60 * 1000).toISOString(),
  })
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata)

  const output = await selfUpdate.handler({
    ...opts,
    minimumReleaseAge: 24 * 60,
    minimumReleaseAgeExclude: ['pnpm'],
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.1.0')
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@9.1.0')
})

test('self-update respects minimumReleaseAgeExclude exact version for implicit latest resolution', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0',
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  const now = Date.now()
  const metadata = createMetadata('9.1.0', opts.registries.default, ['9.0.0'], {
    '9.0.0': new Date(now - 48 * 60 * 60 * 1000).toISOString(),
    '9.1.0': new Date(now - 8 * 60 * 60 * 1000).toISOString(),
  })
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata)

  const output = await selfUpdate.handler({
    ...opts,
    minimumReleaseAge: 24 * 60,
    minimumReleaseAgeExclude: ['pnpm@9.1.0'],
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.1.0')
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@9.1.0')
})

test('self-update does not bypass minimumReleaseAge when minimumReleaseAgeExclude exact version does not match latest', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0',
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  const now = Date.now()
  const metadata = createMetadata('9.1.0', opts.registries.default, ['9.0.0'], {
    '9.0.0': new Date(now - 48 * 60 * 60 * 1000).toISOString(),
    '9.1.0': new Date(now - 8 * 60 * 60 * 1000).toISOString(),
  })
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata)

  const output = await selfUpdate.handler({
    ...opts,
    minimumReleaseAge: 24 * 60,
    minimumReleaseAgeExclude: ['pnpm@9.0.0'],
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@9.0.0')
})

test('self-update throws on invalid minimumReleaseAgeExclude pattern', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0',
  })
  const now = Date.now()
  const metadata = createMetadata('9.1.0', opts.registries.default, ['9.0.0'], {
    '9.0.0': new Date(now - 48 * 60 * 60 * 1000).toISOString(),
    '9.1.0': new Date(now - 8 * 60 * 60 * 1000).toISOString(),
  })
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, metadata)

  await expect(selfUpdate.handler({
    ...opts,
    minimumReleaseAge: 24 * 60,
    minimumReleaseAgeExclude: ['pnpm@^9.0.0'],
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE',
  })
})

test('self-update refuses to downgrade when latest is older than current', async () => {
  const opts = prepare()
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('8.15.0', opts.registries.default))

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe('The currently active pnpm v9.0.0 is newer than the "latest" version on the registry (v8.15.0). No update performed. Run "pnpm self-update latest" to downgrade.')
  // No global install dir should have been created.
  const globalDir = path.join(opts.pnpmHomeDir, 'global', 'v11')
  expect(fs.existsSync(globalDir)).toBe(false)
})

test('self-update latest forces the downgrade even when latest is older', async () => {
  const opts = prepare()
  // Mocked current pnpm is v9.0.0; mocking `latest` as v8.15.0 makes this an
  // actual downgrade so the test exercises the explicit-`latest` bypass of
  // the no-downgrade guard. The fixture tarball is still 9.1.0, but this test
  // only checks that the install path was reached — not the resulting pinned
  // version.
  mockRegistryForUpdate(opts.registries.default, '8.15.0', createMetadata('8.15.0', opts.registries.default))

  const output = await selfUpdate.handler(opts, ['latest'])

  expect(output).not.toMatch(/No update performed/)
  const globalDir = path.join(opts.pnpmHomeDir, 'global', 'v11')
  expect(fs.existsSync(globalDir)).toBe(true)
})

test('self-update by exact older version skips the no-downgrade guard', async () => {
  const opts = prepare()
  // The fixture tarball's actual contents are still 9.1.0; only the registry
  // metadata claims 8.15.0. That is fine here — this test only verifies that
  // an explicit version argument bypasses the implicit-latest guard, not the
  // resulting pinned version.
  mockRegistryForUpdate(opts.registries.default, '8.15.0', createMetadata('8.15.0', opts.registries.default))

  const output = await selfUpdate.handler(opts, ['8.15.0'])

  expect(output).not.toMatch(/No update performed/)
  const globalDir = path.join(opts.pnpmHomeDir, 'global', 'v11')
  expect(fs.existsSync(globalDir)).toBe(true)
})

test('self-update refuses to downgrade the project pin when latest is older', async () => {
  const opts = prepare({
    packageManager: 'pnpm@10.0.0',
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.5.0', opts.registries.default))

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '10.0.0',
    },
  }, [])

  expect(output).toBe('The current project is set to use pnpm v10.0.0, which is newer than the "latest" version on the registry (v9.5.0). No update performed. Run "pnpm self-update latest" to downgrade.')
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@10.0.0')
})

test('self-update refuses to downgrade the project pin when the lockfile is pinned above the range', async () => {
  // Range spec like ">=8.0.0" understates the installed version when the
  // env lockfile has pinned a higher exact version. The guard must consult
  // the lockfile, not just the spec's lower bound.
  const opts = prepare({
    devEngines: {
      packageManager: { name: 'pnpm', version: '>=8.0.0' },
    },
  })
  fs.writeFileSync(path.join(opts.dir, 'pnpm-lock.yaml'), [
    '---',
    "lockfileVersion: '9.0'",
    '',
    'importers:',
    '',
    '  .:',
    '    configDependencies: {}',
    '    packageManagerDependencies:',
    '      pnpm:',
    "        specifier: '>=8.0.0'",
    '        version: 10.5.0',
    '',
    'packages: {}',
    'snapshots: {}',
    '---',
    '',
  ].join('\n'), 'utf8')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.5.0', opts.registries.default))

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '>=8.0.0',
    },
  }, [])

  expect(output).toBe('The current project is set to use pnpm v10.5.0, which is newer than the "latest" version on the registry (v9.5.0). No update performed. Run "pnpm self-update latest" to downgrade.')
})

test('should update packageManager field when a newer pnpm version is available', async () => {
  const opts = prepare()
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  fs.writeFileSync(pkgJsonPath, JSON.stringify({
    packageManager: 'pnpm@8.0.0',
  }), 'utf8')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default))

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@9.0.0')
})

test('should not update packageManager field when current version matches latest', async () => {
  const opts = prepare()
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  fs.writeFileSync(pkgJsonPath, JSON.stringify({
    packageManager: 'pnpm@9.0.0',
  }), 'utf8')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default))

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '9.0.0',
    },
  }, [])

  expect(output).toBe('The current project is already set to use pnpm v9.0.0')
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@9.0.0')
})

test('should update devEngines.packageManager version when a newer pnpm version is available', async () => {
  const opts = prepare({
    devEngines: {
      packageManager: { name: 'pnpm', version: '8.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.devEngines.packageManager.version).toBe('9.0.0')
  expect(pkgJson.packageManager).toBeUndefined()
})

test('should update pnpm entry in devEngines.packageManager array', async () => {
  const opts = prepare({
    devEngines: {
      packageManager: [
        { name: 'npm', version: '10.0.0' },
        { name: 'pnpm', version: '8.0.0' },
      ],
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.devEngines.packageManager[1].version).toBe('9.0.0')
  expect(pkgJson.devEngines.packageManager[0].version).toBe('10.0.0')
  expect(pkgJson.packageManager).toBeUndefined()
})

test('should not modify devEngines.packageManager range when resolved version still satisfies it', async () => {
  const opts = prepare({
    devEngines: {
      packageManager: { name: 'pnpm', version: '>=8.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '>=8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  // The range should remain unchanged — the exact version is pinned in the lockfile
  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.devEngines.packageManager.version).toBe('>=8.0.0')
  // The lockfile should be written with the resolved exact version
  const lockfile = fs.readFileSync(path.join(opts.dir, 'pnpm-lock.yaml'), 'utf8')
  expect(lockfile).toContain('9.0.0')
})

test('should fall back to ^version when complex range cannot accommodate the new version', async () => {
  const opts = prepare({
    devEngines: {
      packageManager: { name: 'pnpm', version: '>=8.0.0 <9.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '>=8.0.0 <9.0.0',
    },
  }, [])

  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.devEngines.packageManager.version).toBe('^9.0.0')
})

test('should update both packageManager and devEngines.packageManager when both pin the same exact version', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0',
    devEngines: {
      packageManager: { name: 'pnpm', version: '8.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.packageManager).toBe('pnpm@9.0.0')
  expect(pkgJson.devEngines.packageManager.version).toBe('9.0.0')
})

test('should update both packageManager (with integrity hash) and devEngines.packageManager when versions agree', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0+sha512.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000',
    devEngines: {
      packageManager: { name: 'pnpm', version: '8.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.packageManager).toBe('pnpm@9.0.0')
  expect(pkgJson.devEngines.packageManager.version).toBe('9.0.0')
})

test('should sync both fields to the new exact version when their current versions disagree', async () => {
  const opts = prepare({
    packageManager: 'pnpm@7.0.0',
    devEngines: {
      packageManager: { name: 'pnpm', version: '8.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '8.0.0',
    },
  }, [])

  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.packageManager).toBe('pnpm@9.0.0')
  expect(pkgJson.devEngines.packageManager.version).toBe('9.0.0')
})

test('should pin devEngines.packageManager to an exact version when packageManager also pins pnpm', async () => {
  const opts = prepare({
    packageManager: 'pnpm@8.0.0',
    devEngines: {
      packageManager: { name: 'pnpm', version: '^8.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '^8.0.0',
    },
  }, [])

  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.packageManager).toBe('pnpm@9.0.0')
  expect(pkgJson.devEngines.packageManager.version).toBe('9.0.0')
})

test('should leave packageManager alone when it pins a different package manager', async () => {
  const opts = prepare({
    packageManager: 'yarn@4.0.0',
    devEngines: {
      packageManager: { name: 'pnpm', version: '^8.0.0' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '^8.0.0',
    },
  }, [])

  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  expect(pkgJson.packageManager).toBe('yarn@4.0.0')
  expect(pkgJson.devEngines.packageManager.version).toBe('^9.0.0')
})

test('should update devEngines.packageManager range when resolved version no longer satisfies it', async () => {
  const opts = prepare({
    devEngines: {
      packageManager: { name: 'pnpm', version: '^8' },
    },
  })
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.0.0')

  const output = await selfUpdate.handler({
    ...opts,
    wantedPackageManager: {
      name: 'pnpm',
      version: '^8',
    },
  }, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.0.0')
  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
  // Range operator preserved, version updated
  expect(pkgJson.devEngines.packageManager.version).toBe('^9.0.0')
})

test('self-update finds pnpm that is already in the global dir', async () => {
  const opts = prepare()
  const globalDir = opts.globalPkgDir

  // Pre-create a pnpm package in the global dir with a hash symlink
  const installDir = path.join(globalDir, 'test-install')
  const pkgDir = path.join(installDir, 'node_modules', 'pnpm')
  fs.mkdirSync(pkgDir, { recursive: true })
  fs.writeFileSync(path.join(installDir, 'package.json'), JSON.stringify({ dependencies: { pnpm: '9.2.0' } }), 'utf8')
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'pnpm', version: '9.2.0', bin: { pnpm: 'bin.js' } }), 'utf8')
  fs.writeFileSync(path.join(pkgDir, 'bin.js'), `#!/usr/bin/env node
console.log('9.2.0')`, 'utf8')
  // Create a hash symlink pointing to the install dir (like handleGlobalAdd does)
  fs.symlinkSync(installDir, path.join(globalDir, 'fake-hash'))

  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.2.0', opts.registries.default)).persist()
  mockExeMetadata(opts.registries.default, '9.2.0')

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe(`The latest version, v9.2.0, is already present on the system. It was activated by linking it from ${installDir}.`)

  const pnpmEnv = prependDirsToPath([path.join(opts.pnpmHomeDir, 'bin')])
  const { status, stdout } = spawn.sync('pnpm', ['-v'], {
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  expect(status).toBe(0)
  expect(stdout.toString().trim()).toBe('9.2.0')
})

test('self-update works globally without package.json', async () => {
  const dir = tempDir(false)
  // No package.json in this directory
  const pnpmHomeDir = path.join(dir, 'pnpm-home')
  fs.mkdirSync(pnpmHomeDir, { recursive: true })
  const opts = {
    ...prepareOptions(dir),
    globalPkgDir: path.join(pnpmHomeDir, 'global', 'v11'),
    pnpmHomeDir,
    bin: path.join(pnpmHomeDir, 'bin'),
  }
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  await selfUpdate.handler(opts, [])

  // Verify no package.json was created
  expect(fs.existsSync(path.join(dir, 'package.json'))).toBe(false)

  // Verify pnpm-lock.yaml was written to pnpmHomeDir
  expect(fs.existsSync(path.join(pnpmHomeDir, 'pnpm-lock.yaml'))).toBe(true)

  // Verify the package was installed in the global dir
  const globalDir = path.join(pnpmHomeDir, 'global', 'v11')
  const globalEntries = fs.readdirSync(globalDir)
  const globalInstallDir = globalEntries.find((e) => fs.statSync(path.join(globalDir, e)).isDirectory())
  expect(globalInstallDir).toBeDefined()
  expect(fs.existsSync(path.join(globalDir, globalInstallDir!, 'node_modules', 'pnpm', 'package.json'))).toBe(true)

  const pnpmEnv = prependDirsToPath([path.join(pnpmHomeDir, 'bin')])
  const { status, stdout } = spawn.sync('pnpm', ['-v'], {
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  expect(status).toBe(0)
  expect(stdout.toString().trim()).toBe('9.1.0')
})

test('self-update updates the packageManager field in package.json', async () => {
  prepareWithPkg({
    packageManager: 'pnpm@9.0.0',
  })
  const opts = {
    ...prepareOptions(process.cwd()),
    wantedPackageManager: {
      name: 'pnpm',
      version: '9.0.0',
    },
  }
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.1.0', opts.registries.default))

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.1.0')

  const pkgJson = JSON.parse(fs.readFileSync(path.resolve('package.json'), 'utf8'))
  expect(pkgJson.packageManager).toBe('pnpm@9.1.0')
})

test('installPnpm rejects and cleans up when the installed pnpm has no working executable', async () => {
  const opts = prepare()
  // A package with no bin stands in for a release whose executable is missing.
  // Serving it as pnpm's tarball is the only way here without publishing a
  // broken pnpm as a fixture, so the metadata carries this tarball's integrity.
  const tgzWithoutBin = fs.readFileSync(require.resolve('@pnpm/tgz-fixtures/tgz/is-positive-1.0.0.tgz'))
  const registry = opts.registries.default
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, {
      name: 'pnpm',
      'dist-tags': { latest: '9.1.0' },
      versions: {
        '9.1.0': {
          name: 'pnpm',
          version: '9.1.0',
          dist: {
            tarball: `${registry}pnpm/-/pnpm-9.1.0.tgz`,
            integrity: `sha512-${createHash('sha512').update(tgzWithoutBin).digest('base64')}`,
          },
        },
      },
      time: {},
    }).persist()
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: '/pnpm/-/pnpm-9.1.0.tgz', method: 'GET' })
    .reply(200, tgzWithoutBin)

  await expect(installPnpm('9.1.0', opts)).rejects.toThrow(/cannot run/)

  // The half-installed directory must not survive: leaving it behind would let
  // findGlobalPnpmInstallDir hand the broken install to the next run.
  const installDirs = fs.existsSync(opts.globalPkgDir)
    ? fs.readdirSync(opts.globalPkgDir).filter((entry) => /^\d+$/.test(entry))
    : []
  expect(installDirs).toHaveLength(0)
})

test('installPnpm without env lockfile uses resolution path', async () => {
  const opts = prepare()
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.1.0', opts.registries.default)).persist()
  const tgzData = fs.readFileSync(pnpmTarballPath)
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm/-/pnpm-9.1.0.tgz', method: 'GET' })
    .reply(200, tgzData)

  const result = await installPnpm('9.1.0', opts)

  expect(result.alreadyExisted).toBe(false)
  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(result.baseDir, 'node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')
  expect(fs.existsSync(result.binDir)).toBe(true)
})

describe('linkExePlatformBinary', () => {
  const platform = process.platform
  const arch = platform === 'win32' && process.arch === 'ia32' ? 'x86' : process.arch
  const executable = platform === 'win32' ? 'pnpm.exe' : 'pnpm'
  // Match the libc family linkExePlatformBinary detects at runtime so the
  // fixture directory matches what the implementation looks up, including on
  // musl hosts (Alpine CI).
  const libcFamily = familySync()
  const platformPkgName = exePlatformPkgDirName(platform, arch, libcFamily)

  test('prefers the wrapper-adjacent platform binary in a symlinked node_modules layout', () => {
    const dir = tempDir(false)

    // Create a virtual store layout like pnpm produces:
    //   .pnpm/@pnpm+exe@1.0.0/node_modules/@pnpm/exe/     (the real @pnpm/exe dir)
    //   .pnpm/@pnpm+exe@1.0.0/node_modules/@pnpm/<platform>-<arch>/  (platform binary)
    //   node_modules/@pnpm/exe -> symlink to the virtual store entry
    const vsExeDir = path.join(dir, 'node_modules', '.pnpm', '@pnpm+exe@1.0.0', 'node_modules', '@pnpm', 'exe')
    const vsPlatformDir = path.join(dir, 'node_modules', '.pnpm', '@pnpm+exe@1.0.0', 'node_modules', '@pnpm', platformPkgName)
    const topLevelExeDir = path.join(dir, 'node_modules', '@pnpm', 'exe')
    const topLevelPlatformDir = path.join(dir, 'node_modules', '@pnpm', platformPkgName)

    // Create the virtual store directories
    fs.mkdirSync(vsExeDir, { recursive: true })
    fs.mkdirSync(vsPlatformDir, { recursive: true })

    // Write the placeholder file (as published in the @pnpm/exe tarball)
    fs.writeFileSync(path.join(vsExeDir, executable), 'This file intentionally left blank')
    // Write a package.json (needed on Windows where bin.pnpm is rewritten to pnpm.exe)
    fs.writeFileSync(path.join(vsExeDir, 'package.json'), JSON.stringify({ bin: { pnpm: 'pnpm' } }))

    // Write a fake platform binary
    const fakeBinaryContent = '#!/bin/sh\necho "fake pnpm binary"'
    fs.writeFileSync(path.join(vsPlatformDir, executable), fakeBinaryContent)

    // Create the top-level symlink: node_modules/@pnpm/exe -> virtual store
    fs.mkdirSync(path.join(dir, 'node_modules', '@pnpm'), { recursive: true })
    fs.symlinkSync(vsExeDir, topLevelExeDir)
    fs.mkdirSync(topLevelPlatformDir)
    fs.writeFileSync(path.join(topLevelPlatformDir, executable), 'wrong platform binary')

    // Run the function
    linkExePlatformBinary(dir)

    // The placeholder should be replaced with the platform binary content
    const result = fs.readFileSync(path.join(topLevelExeDir, executable), 'utf8')
    expect(result).toBe(fakeBinaryContent)

    // pn is a shell script in the tarball (not created by linkExePlatformBinary)
  })

  test('also works with flat node_modules layout', () => {
    const dir = tempDir(false)

    // In a flat layout (no symlinks), both packages are at the top level
    const exeDir = path.join(dir, 'node_modules', '@pnpm', 'exe')
    const platformDir = path.join(dir, 'node_modules', '@pnpm', platformPkgName)

    fs.mkdirSync(exeDir, { recursive: true })
    fs.mkdirSync(platformDir, { recursive: true })

    fs.writeFileSync(path.join(exeDir, executable), 'This file intentionally left blank')
    // Write a package.json (needed on Windows where bin.pnpm is rewritten to pnpm.exe)
    fs.writeFileSync(path.join(exeDir, 'package.json'), JSON.stringify({ bin: { pnpm: 'pnpm' } }))

    const fakeBinaryContent = '#!/bin/sh\necho "fake pnpm binary"'
    fs.writeFileSync(path.join(platformDir, executable), fakeBinaryContent)

    linkExePlatformBinary(dir)

    const result = fs.readFileSync(path.join(exeDir, executable), 'utf8')
    expect(result).toBe(fakeBinaryContent)
  })

  test('does nothing when @pnpm/exe is not installed', () => {
    const dir = tempDir(false)
    fs.mkdirSync(path.join(dir, 'node_modules'), { recursive: true })

    // Should not throw
    linkExePlatformBinary(dir)
  })

  test('does nothing when platform binary is not available', () => {
    const dir = tempDir(false)
    const exeDir = path.join(dir, 'node_modules', '@pnpm', 'exe')
    fs.mkdirSync(exeDir, { recursive: true })

    const placeholder = 'This file intentionally left blank'
    fs.writeFileSync(path.join(exeDir, executable), placeholder)

    linkExePlatformBinary(dir)

    // Placeholder should remain unchanged
    const result = fs.readFileSync(path.join(exeDir, executable), 'utf8')
    expect(result).toBe(placeholder)
  })

  test('falls back to future exe.<platform>-<arch> naming scheme', () => {
    const dir = tempDir(false)

    // Simulate a future release where only the new-scheme platform package
    // directory exists under the virtual store — the legacy name is absent.
    const nextPkgName = exePlatformPkgDirNameNext(platform, arch, libcFamily)
    const exeDir = path.join(dir, 'node_modules', '@pnpm', 'exe')
    const platformDir = path.join(dir, 'node_modules', '@pnpm', nextPkgName)

    fs.mkdirSync(exeDir, { recursive: true })
    fs.mkdirSync(platformDir, { recursive: true })

    fs.writeFileSync(path.join(exeDir, executable), 'This file intentionally left blank')
    fs.writeFileSync(path.join(exeDir, 'package.json'), JSON.stringify({ bin: { pnpm: 'pnpm' } }))

    const fakeBinaryContent = '#!/bin/sh\necho "fake pnpm binary"'
    fs.writeFileSync(path.join(platformDir, executable), fakeBinaryContent)

    linkExePlatformBinary(dir)

    const result = fs.readFileSync(path.join(exeDir, executable), 'utf8')
    expect(result).toBe(fakeBinaryContent)
  })

  test('links the pnpm v12 wrapper from its @pnpm/exe.<target> dependency', () => {
    const dir = tempDir(false)

    // pnpm v12 (the Rust port) is published as the unscoped `pnpm` wrapper that
    // depends on `@pnpm/exe.<platform>-<arch>[-musl]` — the `exe.<...>` scheme.
    const nextPkgName = exePlatformPkgDirNameNext(platform, arch, libcFamily)
    const wrapperDir = path.join(dir, 'node_modules', 'pnpm')
    const platformDir = path.join(dir, 'node_modules', '@pnpm', nextPkgName)

    fs.mkdirSync(wrapperDir, { recursive: true })
    fs.mkdirSync(platformDir, { recursive: true })

    fs.writeFileSync(path.join(wrapperDir, executable), 'This is a placeholder.')
    fs.writeFileSync(path.join(wrapperDir, 'package.json'), JSON.stringify({
      bin: { pnpm: 'pnpm', pn: 'pn', pnpx: 'pnpx', pnx: 'pnx' },
    }))

    const fakeBinaryContent = '#!/bin/sh\necho "fake pnpm v12 binary"'
    fs.writeFileSync(path.join(platformDir, executable), fakeBinaryContent)

    linkExePlatformBinary(dir, 'pnpm')

    const result = fs.readFileSync(path.join(wrapperDir, executable), 'utf8')
    expect(result).toBe(fakeBinaryContent)
  })

  test.each([
    ['legacy', platformPkgName],
    ['newer', exePlatformPkgDirNameNext(platform, arch, libcFamily)],
  ])('links @pnpm/exe when its %s platform dependency is in a sibling GVS slot', (_scheme, siblingPlatformPkgName) => {
    const dir = tempDir(false)
    const wrapperSlot = path.join(dir, 'links', 'wrapper', 'node_modules', '@pnpm', 'exe')
    const platformSlot = path.join(dir, 'links', 'platform', 'node_modules', '@pnpm', siblingPlatformPkgName)
    const wrapperDir = path.join(dir, 'node_modules', '@pnpm', 'exe')
    const platformDir = path.join(dir, 'node_modules', '@pnpm', siblingPlatformPkgName)

    fs.mkdirSync(wrapperSlot, { recursive: true })
    fs.mkdirSync(platformSlot, { recursive: true })
    fs.writeFileSync(path.join(wrapperSlot, executable), 'This file intentionally left blank')
    fs.writeFileSync(path.join(wrapperSlot, 'package.json'), JSON.stringify({
      bin: { pnpm: 'pnpm', pn: 'pn', pnpx: 'pnpx', pnx: 'pnx' },
    }))
    const fakeBinaryContent = '#!/bin/sh\necho "fake pnpm binary"'
    fs.writeFileSync(path.join(platformSlot, executable), fakeBinaryContent)

    fs.mkdirSync(path.dirname(wrapperDir), { recursive: true })
    fs.mkdirSync(path.dirname(platformDir), { recursive: true })
    fs.symlinkSync(wrapperSlot, wrapperDir)
    fs.symlinkSync(platformSlot, platformDir)

    linkExePlatformBinary(dir)

    const result = fs.readFileSync(path.join(wrapperDir, executable), 'utf8')
    expect(result).toBe(fakeBinaryContent)
  })

  test('runs the self-update wrapper when its platform binary is in a sibling GVS slot', () => {
    const dir = tempDir(false)
    const wrapperSlot = path.join(dir, 'links', 'wrapper', 'node_modules', '@pnpm', 'exe')
    const platformSlot = path.join(dir, 'links', 'platform', 'node_modules', '@pnpm', platformPkgName)
    const wrapperDir = path.join(dir, 'node_modules', '@pnpm', 'exe')
    const platformDir = path.join(dir, 'node_modules', '@pnpm', platformPkgName)

    fs.mkdirSync(wrapperSlot, { recursive: true })
    fs.mkdirSync(platformSlot, { recursive: true })
    fs.writeFileSync(path.join(wrapperSlot, executable), 'This file intentionally left blank')
    fs.writeFileSync(path.join(wrapperSlot, 'package.json'), JSON.stringify({
      bin: { pnpm: 'pnpm', pn: 'pn', pnpx: 'pnpx', pnx: 'pnx' },
    }))
    fs.copyFileSync(fs.realpathSync(process.execPath), path.join(platformSlot, executable))

    fs.mkdirSync(path.dirname(wrapperDir), { recursive: true })
    fs.mkdirSync(path.dirname(platformDir), { recursive: true })
    fs.symlinkSync(wrapperSlot, wrapperDir)
    fs.symlinkSync(platformSlot, platformDir)

    linkExePlatformBinary(dir)

    const result = spawn.sync(path.join(wrapperDir, executable), ['--version'])
    expect(result.status).toBe(0)
    expect(result.stdout.toString().trim()).toBe(process.version)
  })

  // Regression coverage for https://github.com/pnpm/pnpm/issues/11486 — the
  // `pn` / `pnpx` / `pnx` aliases were broken in MSYS2 / Git Bash on Windows.
  // Root cause: linkExePlatformBinary pointed those bin entries at .cmd files,
  // and @zkochan/cmd-shim's Bash shim for a .cmd source bounces through
  // `exec cmd /C "...target.cmd" "$@"`. MSYS2's argument-conversion runtime
  // mangles the lone `/C` switch into a Windows path before cmd.exe sees it,
  // so cmd.exe finds no /C or /K and falls into interactive mode (printing its
  // banner instead of running the alias). Routing the aliases through .exe
  // hardlinks of the SEA binary takes cmd.exe out of the chain entirely.
  const winOnlyTest = platform === 'win32' ? test : test.skip
  winOnlyTest('rewrites bin to .exe entries and hardlinks pn/pnpx/pnx aliases to pnpm.exe (issue #11486)', () => {
    const dir = tempDir(false)
    const exeDir = path.join(dir, 'node_modules', '@pnpm', 'exe')
    const platformDir = path.join(dir, 'node_modules', '@pnpm', platformPkgName)

    fs.mkdirSync(exeDir, { recursive: true })
    fs.mkdirSync(platformDir, { recursive: true })

    fs.writeFileSync(path.join(exeDir, executable), 'This file intentionally left blank')
    // Match the published bin field from pnpm/artifacts/exe/package.json
    fs.writeFileSync(path.join(exeDir, 'package.json'), JSON.stringify({
      bin: { pnpm: 'pnpm', pn: 'pn', pnpx: 'pnpx', pnx: 'pnx' },
    }))

    // The platform binary needs to be a real file so fs.linkSync can hardlink
    // it. Content doesn't matter.
    fs.writeFileSync(path.join(platformDir, executable), 'fake-pnpm-exe')

    linkExePlatformBinary(dir)

    const rewritten = JSON.parse(fs.readFileSync(path.join(exeDir, 'package.json'), 'utf8'))
    expect(rewritten.bin).toEqual({
      pnpm: 'pnpm.exe',
      pn: 'pn.exe',
      pnpx: 'pnpx.exe',
      pnx: 'pnx.exe',
    })

    const pnpmIno = fs.statSync(path.join(exeDir, 'pnpm.exe')).ino
    for (const name of ['pn', 'pnpx', 'pnx']) {
      const aliasPath = path.join(exeDir, `${name}.exe`)
      expect(fs.existsSync(aliasPath)).toBe(true)
      // Hardlinked to pnpm.exe, so the SEA's argv[0] basename detection can
      // tell `pnpx` apart from `pnpm` and inject `dlx` accordingly.
      expect(fs.statSync(aliasPath).ino).toBe(pnpmIno)
    }
  })
})

describe('pnpmPackageNameToInstall', () => {
  test('installs the unscoped `pnpm` package from v12 onward', () => {
    expect(pnpmPackageNameToInstall('12.0.0-alpha.0')).toBe('pnpm')
    expect(pnpmPackageNameToInstall('12.3.4')).toBe('pnpm')
    expect(pnpmPackageNameToInstall('13.0.0')).toBe('pnpm')
  })

  test('keeps the running package identity before v12', () => {
    // getCurrentPackageName() is `pnpm` in the (non-SEA) test runtime, so this
    // asserts v11 and earlier are not forced onto a different package.
    expect(pnpmPackageNameToInstall('11.9.0')).toBe('pnpm')
    expect(pnpmPackageNameToInstall('9.1.0')).toBe('pnpm')
  })
})

describe('exePlatformPkgDirName', () => {
  test('uses linuxstatic- prefix for linux + musl libc family', () => {
    expect(exePlatformPkgDirName('linux', 'x64', 'musl')).toBe('linuxstatic-x64')
    expect(exePlatformPkgDirName('linux', 'arm64', 'musl')).toBe('linuxstatic-arm64')
  })

  test('uses linux- prefix when libc is glibc or unknown', () => {
    expect(exePlatformPkgDirName('linux', 'x64', 'glibc')).toBe('linux-x64')
    expect(exePlatformPkgDirName('linux', 'arm64', null)).toBe('linux-arm64')
  })

  test('libc is irrelevant on non-linux platforms', () => {
    expect(exePlatformPkgDirName('darwin', 'arm64', 'musl')).toBe('macos-arm64')
    expect(exePlatformPkgDirName('darwin', 'x64', null)).toBe('macos-x64')
    expect(exePlatformPkgDirName('win32', 'x64', 'musl')).toBe('win-x64')
  })

  test('normalizes ia32 to x86 on win32 only', () => {
    expect(exePlatformPkgDirName('win32', 'ia32', null)).toBe('win-x86')
    expect(exePlatformPkgDirName('linux', 'ia32', null)).toBe('linux-ia32')
  })
})

describe('exePlatformPkgDirNameNext', () => {
  test('appends -musl for linux + musl libc family', () => {
    expect(exePlatformPkgDirNameNext('linux', 'x64', 'musl')).toBe('exe.linux-x64-musl')
    expect(exePlatformPkgDirNameNext('linux', 'arm64', 'musl')).toBe('exe.linux-arm64-musl')
  })

  test('does not append -musl when libc is glibc or unknown', () => {
    expect(exePlatformPkgDirNameNext('linux', 'x64', 'glibc')).toBe('exe.linux-x64')
    expect(exePlatformPkgDirNameNext('linux', 'arm64', null)).toBe('exe.linux-arm64')
  })

  test('libc is irrelevant on non-linux platforms', () => {
    expect(exePlatformPkgDirNameNext('darwin', 'arm64', 'musl')).toBe('exe.darwin-arm64')
    expect(exePlatformPkgDirNameNext('darwin', 'x64', null)).toBe('exe.darwin-x64')
    expect(exePlatformPkgDirNameNext('win32', 'x64', 'musl')).toBe('exe.win32-x64')
  })

  test('normalizes ia32 to x86 on win32 only', () => {
    expect(exePlatformPkgDirNameNext('win32', 'ia32', null)).toBe('exe.win32-x86')
    expect(exePlatformPkgDirNameNext('linux', 'ia32', null)).toBe('exe.linux-ia32')
  })
})

describe('assertReleaseIsInstallable', () => {
  test.each(['11.12.0', '11.13.0'])('refuses the broken release %s', (version) => {
    expect(() => {
      assertReleaseIsInstallable(version)
    }).toThrow(/pnpm v.+ is a broken release and cannot be installed/)
  })

  test('the refusal explains that the pin would break teammates', () => {
    try {
      assertReleaseIsInstallable('11.13.0')
      throw new Error('assertReleaseIsInstallable should have thrown')
    } catch (err: unknown) {
      const pnpmError = err as PnpmError
      expect(pnpmError.code).toBe('ERR_PNPM_BROKEN_PNPM_RELEASE')
      expect(pnpmError.hint).toMatch(/pin is shared/)
    }
  })

  test.each(['11.11.0', '11.13.1', '12.0.0'])('allows %s', (version) => {
    expect(() => {
      assertReleaseIsInstallable(version)
    }).not.toThrow()
  })
})

describe('assertPnpmRuns', () => {
  // Build the bins the same way installPnpmToGlobalDir does, so the spawn goes
  // through the real shim linkBins writes — including the .cmd wrapper on
  // Windows, which is the part a hand-written fixture would get wrong.
  async function linkFakePnpm (entry: string): Promise<string> {
    const dir = tempDir(false)
    const pkgDir = path.join(dir, 'node_modules', 'fake-pnpm')
    fs.mkdirSync(pkgDir, { recursive: true })
    fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({
      name: 'fake-pnpm',
      version: '1.0.0',
      bin: { pnpm: 'entry.js' },
    }))
    fs.writeFileSync(path.join(pkgDir, 'entry.js'), `${entry}\n`)
    const binDir = path.join(dir, 'bin')
    await linkBins(path.join(dir, 'node_modules'), binDir, { warn: () => {} })
    return binDir
  }

  test('passes when the installed pnpm runs', async () => {
    const binDir = await linkFakePnpm('process.exit(0)')
    expect(() => {
      assertPnpmRuns(binDir, '1.0.0')
    }).not.toThrow()
  })

  test('reports the failing exit code when the installed pnpm cannot run', async () => {
    const binDir = await linkFakePnpm('process.exit(1)')
    expect(() => {
      assertPnpmRuns(binDir, '1.0.0')
    }).toThrow(/pnpm v1\.0\.0 that was just installed cannot run.*exited with code 1/s)
  })

  // Windows has no real signals, so a killed process still reports an exit code
  // there and this branch is unreachable.
  const posixOnlyTest = process.platform === 'win32' ? test.skip : test

  posixOnlyTest('describes a signal rather than a null exit code', async () => {
    // macOS kills a binary whose signature check rejects it, so an incorrectly signed
    // release arrives here with no exit code at all. The wording matches
    // pacquet's, which only has the code and cannot name the signal.
    const binDir = await linkFakePnpm("process.kill(process.pid, 'SIGKILL')")
    expect(() => {
      assertPnpmRuns(binDir, '1.0.0')
    }).toThrow(/cannot run: it exited with a signal/)
  })

  // pnpm reaches --version only after loading config and running pnpmfile
  // hooks, so probing from the caller's directory would let an unrelated
  // project reject a perfectly good release.
  test('is not affected by the config of the directory self-update was run from', async () => {
    // Stands in for pnpm's startup: fail if the caller's project is visible.
    const binDir = await linkFakePnpm("process.exit(require('node:fs').existsSync('.pnpmfile.cjs') ? 1 : 0)")
    const hostileProject = tempDir(false)
    fs.writeFileSync(path.join(hostileProject, 'package.json'), '{"name":"p"}')
    fs.writeFileSync(path.join(hostileProject, 'pnpm-workspace.yaml'), 'packages:\n  - .\n')
    fs.writeFileSync(path.join(hostileProject, '.pnpmfile.cjs'), "throw new Error('broken pnpmfile')\n")
    const cwd = process.cwd()
    process.chdir(hostileProject)
    try {
      expect(() => {
        assertPnpmRuns(binDir, '1.0.0')
      }).not.toThrow()
    } finally {
      process.chdir(cwd)
    }
  })

  test('fails when there is no pnpm to run at all', () => {
    expect(() => {
      assertPnpmRuns(tempDir(false), '1.0.0')
    }).toThrow(/cannot run/)
  })

  test('the failure carries the BROKEN_PNPM_INSTALL code and says the active pnpm was kept', async () => {
    const binDir = await linkFakePnpm('process.exit(1)')
    try {
      assertPnpmRuns(binDir, '1.0.0')
      throw new Error('assertPnpmRuns should have thrown')
    } catch (err: unknown) {
      const pnpmError = err as PnpmError
      expect(pnpmError.code).toBe('ERR_PNPM_BROKEN_PNPM_INSTALL')
      expect(pnpmError.hint).toMatch(/currently active pnpm was left in place/)
    }
  })
})
