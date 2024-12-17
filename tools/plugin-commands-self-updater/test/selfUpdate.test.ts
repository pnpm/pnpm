import fs from 'fs'
import path from 'path'
import { prependDirsToPath } from '@pnpm/env.path'
import { tempDir, prepare as prepareWithPkg } from '@pnpm/prepare'
import { selfUpdate } from '@pnpm/tools.plugin-commands-self-updater'
import spawn from 'cross-spawn'
import nock from 'nock'

const pnpmTarballPath = require.resolve('@pnpm/tgz-fixtures/tgz/pnpm-9.1.0.tgz')

jest.mock('@pnpm/cli-meta', () => {
  const actualModule = jest.requireActual('@pnpm/cli-meta')

  return {
    ...actualModule,
    packageManager: {
      name: 'pnpm',
      version: '9.0.0',
    },
  }
})

afterEach(() => {
  nock.cleanAll()
  nock.disableNetConnect()
})

beforeEach(() => {
  nock.enableNetConnect()
})

function prepare () {
  const dir = tempDir(false)
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
    pnpmHomeDir: dir,
    preferWorkspacePackages: true,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    rawLocalConfig: {},
    sort: false,
    rootProjectManifestDir: process.cwd(),
    bin: process.cwd(),
    workspaceConcurrency: 1,
    extraEnv: {},
    pnpmfile: '',
    rawConfig: {},
    cacheDir: path.join(dir, '.cache'),
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    dir: process.cwd(),
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

test('self-update', async () => {
  const opts = prepare()
  nock(opts.registries.default)
    .get('/pnpm')
    .reply(200, createMetadata('9.1.0', opts.registries.default))
  nock(opts.registries.default)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, pnpmTarballPath)

  await selfUpdate.handler(opts, [])

  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(opts.pnpmHomeDir, '.tools/pnpm/9.1.0/node_modules/pnpm/package.json'), 'utf8'))
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
  nock(opts.registries.default)
    .get('/pnpm')
    .reply(200, createMetadata('9.2.0', opts.registries.default, ['9.1.0']))
  nock(opts.registries.default)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, pnpmTarballPath)

  await selfUpdate.handler(opts, ['9.1.0'])

  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(opts.pnpmHomeDir, '.tools/pnpm/9.1.0/node_modules/pnpm/package.json'), 'utf8'))
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

test('self-update links pnpm that is already present on the disk', async () => {
  const opts = prepare()
  nock(opts.registries.default)
    .get('/pnpm')
    .reply(200, createMetadata('9.2.0', opts.registries.default))

  const latestPnpmDir = path.join(opts.pnpmHomeDir, '.tools/pnpm/9.2.0/node_modules/pnpm')
  fs.mkdirSync(latestPnpmDir, { recursive: true })
  fs.writeFileSync(path.join(latestPnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', bin: 'bin.js' }), 'utf8')
  fs.writeFileSync(path.join(latestPnpmDir, 'bin.js'), `#!/usr/bin/env node
console.log('9.2.0')`, 'utf8')
  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe(`The latest version, v9.2.0, is already present on the system. It was activated by linking it from ${path.join(latestPnpmDir, '../..')}.`)

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
    .get('/pnpm')
    .reply(200, createMetadata('9.1.0', opts.registries.default))
  nock(opts.registries.default)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, pnpmTarballPath)

  const output = await selfUpdate.handler(opts, [])

  expect(output).toBe('The current project has been updated to use pnpm v9.1.0')

  const pkgJson = JSON.parse(fs.readFileSync(path.resolve('package.json'), 'utf8'))
  expect(pkgJson.packageManager).toBe('pnpm@9.1.0')
})
