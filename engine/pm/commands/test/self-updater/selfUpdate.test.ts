import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import { jest } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { prepare as prepareWithPkg, tempDir } from '@pnpm/prepare'
import { prependDirsToPath } from '@pnpm/shell.path'
import { getRegisteredProjects } from '@pnpm/store.controller'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import spawn from 'cross-spawn'

const require = createRequire(import.meta.dirname)
const pnpmTarballPath = require.resolve('@pnpm/tgz-fixtures/tgz/pnpm-9.1.0.tgz')

const actualModule = await import('@pnpm/cli.meta')
jest.unstable_mockModule('@pnpm/cli.meta', () => {
  return {
    ...actualModule,
    packageManager: {
      name: 'pnpm',
      version: '9.0.0',
    },
  }
})
const { selfUpdate, installPnpm, linkExePlatformBinary } = await import('@pnpm/engine.pm.commands')

beforeEach(async () => {
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
    managePackageManagerVersions: false,
  }
}

function createMetadata (latest: string, registry: string, otherVersions: string[] = []) {
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
  getMockAgent().get(opts.registries.default.replace(/\/$/, ''))
    .intercept({ path: '/pnpm', method: 'GET' })
    .reply(200, createMetadata('9.0.0', opts.registries.default))

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe('The currently active pnpm v9.0.0 is already "latest" and doesn\'t need an update')
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
    managePackageManagerVersions: true,
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
    managePackageManagerVersions: true,
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
    managePackageManagerVersions: true,
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
    managePackageManagerVersions: true,
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
    managePackageManagerVersions: true,
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
    managePackageManagerVersions: true,
    wantedPackageManager: {
      name: 'pnpm',
      version: '>=8.0.0 <9.0.0',
    },
  }, [])

  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
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
    managePackageManagerVersions: true,
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
    managePackageManagerVersions: true,
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
  const platform = process.platform === 'win32'
    ? 'win'
    : process.platform === 'darwin'
      ? 'macos'
      : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  const executable = platform === 'win' ? 'pnpm.exe' : 'pnpm'
  const platformPkgName = `${platform}-${arch}`

  test('links platform binary in pnpm symlinked node_modules layout', () => {
    const dir = tempDir(false)

    // Create a virtual store layout like pnpm produces:
    //   .pnpm/@pnpm+exe@1.0.0/node_modules/@pnpm/exe/     (the real @pnpm/exe dir)
    //   .pnpm/@pnpm+exe@1.0.0/node_modules/@pnpm/<platform>-<arch>/  (platform binary)
    //   node_modules/@pnpm/exe -> symlink to the virtual store entry
    const vsExeDir = path.join(dir, 'node_modules', '.pnpm', '@pnpm+exe@1.0.0', 'node_modules', '@pnpm', 'exe')
    const vsPlatformDir = path.join(dir, 'node_modules', '.pnpm', '@pnpm+exe@1.0.0', 'node_modules', '@pnpm', platformPkgName)
    const topLevelExeDir = path.join(dir, 'node_modules', '@pnpm', 'exe')

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
})
