import fs from 'fs'
import { createRequire } from 'module'
import path from 'path'
import { prependDirsToPath } from '@pnpm/env.path'
import { tempDir, prepare as prepareWithPkg } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import spawn from 'cross-spawn'
import nock from 'nock'

const require = createRequire(import.meta.dirname)
const pnpmTarballPath = require.resolve('@pnpm/tgz-fixtures/tgz/pnpm-9.1.0.tgz')

const actualModule = await import('@pnpm/cli-meta')
jest.unstable_mockModule('@pnpm/cli-meta', () => {
  return {
    ...actualModule,
    packageManager: {
      name: 'pnpm',
      version: '9.0.0',
    },
  }
})
const { selfUpdate, installPnpmToTools } = await import('@pnpm/tools.plugin-commands-self-updater')

afterEach(() => {
  nock.cleanAll()
  nock.disableNetConnect()
})

beforeEach(() => {
  nock.enableNetConnect()
})

function prepare () {
  const dir = tempDir(false)
  fs.writeFileSync(path.join(dir, 'package.json'), '{}', 'utf8')
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
    rawLocalConfig: {},
    sort: false,
    rootProjectManifestDir: dir,
    bin: dir,
    workspaceConcurrency: 1,
    extraEnv: {},
    pnpmfile: '',
    rawConfig: {},
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
  nock(registry)
    .get('/@pnpm%2Fexe') // cspell:disable-line
    .reply(200, createExeMetadata(version, registry))
}

/**
 * Mock all registry requests needed for a full self-update flow.
 * This includes: initial resolution, resolvePackageManagerIntegrities, and handleGlobalAdd.
 */
function mockRegistryForUpdate (registry: string, version: string, metadata: object) {
  // Use persist for metadata since multiple components request it
  nock(registry)
    .persist()
    .get('/pnpm')
    .reply(200, metadata)
  mockExeMetadata(registry, version)
  nock(registry)
    .get(`/pnpm/-/pnpm-${version}.tgz`)
    .replyWithFile(200, pnpmTarballPath)
}

test('self-update', async () => {
  const opts = prepare()
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  await selfUpdate.handler(opts, [])

  // Verify the package was installed in the global dir
  const globalDir = path.join(opts.pnpmHomeDir, 'global', 'v11')
  const entries = fs.readdirSync(globalDir)
  const installDirName = entries.find((e) => fs.statSync(path.join(globalDir, e)).isDirectory())
  expect(installDirName).toBeDefined()
  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(globalDir, installDirName!, 'node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')

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

test('self-update by exact version', async () => {
  const opts = prepare()
  const metadata = createMetadata('9.2.0', opts.registries.default, ['9.1.0'])
  nock(opts.registries.default)
    .persist()
    .get('/pnpm')
    .reply(200, metadata)
  mockExeMetadata(opts.registries.default, '9.1.0')
  nock(opts.registries.default)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, pnpmTarballPath)

  await selfUpdate.handler(opts, ['9.1.0'])

  // Verify the package was installed in the global dir
  const globalDir = path.join(opts.pnpmHomeDir, 'global', 'v11')
  const entries = fs.readdirSync(globalDir)
  const installDirName = entries.find((e) => fs.statSync(path.join(globalDir, e)).isDirectory())
  expect(installDirName).toBeDefined()
  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(globalDir, installDirName!, 'node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')

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

test('self-update does nothing when pnpm is up to date', async () => {
  const opts = prepare()
  nock(opts.registries.default)
    .get('/pnpm')
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
  nock(opts.registries.default)
    .persist()
    .get('/pnpm')
    .reply(200, createMetadata('9.0.0', opts.registries.default))
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
  expect(JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).packageManager).toBe('pnpm@9.0.0')
})

test('should not update packageManager field when current version matches latest', async () => {
  const opts = prepare()
  const pkgJsonPath = path.join(opts.dir, 'package.json')
  fs.writeFileSync(pkgJsonPath, JSON.stringify({
    packageManager: 'pnpm@9.0.0',
  }), 'utf8')
  nock(opts.registries.default)
    .get('/pnpm')
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

  nock(opts.registries.default)
    .persist()
    .get('/pnpm')
    .reply(200, createMetadata('9.2.0', opts.registries.default))
  mockExeMetadata(opts.registries.default, '9.2.0')

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe(`The latest version, v9.2.0, is already present on the system. It was activated by linking it from ${installDir}.`)

  const pnpmEnv = prependDirsToPath([opts.pnpmHomeDir])
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
    bin: pnpmHomeDir,
  }
  mockRegistryForUpdate(opts.registries.default, '9.1.0', createMetadata('9.1.0', opts.registries.default))

  await selfUpdate.handler(opts, [])

  // Verify no package.json was created
  expect(fs.existsSync(path.join(dir, 'package.json'))).toBe(false)

  // Verify pnpm-lock.env.yaml was written to pnpmHomeDir
  expect(fs.existsSync(path.join(pnpmHomeDir, 'pnpm-lock.env.yaml'))).toBe(true)

  // Verify the package was installed in the global dir
  const globalDir = path.join(pnpmHomeDir, 'global', 'v11')
  const globalEntries = fs.readdirSync(globalDir)
  const globalInstallDir = globalEntries.find((e) => fs.statSync(path.join(globalDir, e)).isDirectory())
  expect(globalInstallDir).toBeDefined()
  expect(fs.existsSync(path.join(globalDir, globalInstallDir!, 'node_modules', 'pnpm', 'package.json'))).toBe(true)

  const pnpmEnv = prependDirsToPath([pnpmHomeDir])
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
  nock(opts.registries.default)
    .persist()
    .get('/pnpm')
    .reply(200, createMetadata('9.1.0', opts.registries.default))
  mockExeMetadata(opts.registries.default, '9.1.0')
  nock(opts.registries.default)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, pnpmTarballPath)

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.1.0')

  const pkgJson = JSON.parse(fs.readFileSync(path.resolve('package.json'), 'utf8'))
  expect(pkgJson.packageManager).toBe('pnpm@9.1.0')
})

test('installPnpmToTools without env lockfile uses resolution path', async () => {
  const opts = prepare()
  nock(opts.registries.default)
    .persist()
    .get('/pnpm')
    .reply(200, createMetadata('9.1.0', opts.registries.default))
  nock(opts.registries.default)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, pnpmTarballPath)

  const result = await installPnpmToTools('9.1.0', opts)

  expect(result.alreadyExisted).toBe(false)
  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(result.baseDir, 'node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')
  expect(fs.existsSync(result.binDir)).toBe(true)
})
