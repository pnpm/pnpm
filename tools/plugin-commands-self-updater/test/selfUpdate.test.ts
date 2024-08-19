import fs from 'fs'
import path from 'path'
import { prependDirsToPath } from '@pnpm/env.path'
import { tempDir } from '@pnpm/prepare'
import { selfUpdate } from '@pnpm/tools.plugin-commands-self-updater'
import spawn from 'cross-spawn'
import nock from 'nock'

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
  return {
    argv: {
      original: [],
    },
    cliOptions: {},
    linkWorkspacePackages: true,
    bail: true,
    pnpmHomeDir: dir,
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
    virtualStoreDirMaxLength: 120,
    dir: process.cwd(),
  }
}

test('self-update', async () => {
  const opts = prepare()
  nock(opts.registries.default)
    .get('/pnpm')
    .reply(200, {
      name: 'pnpm',
      'dist-tags': {
        latest: '9.1.0',
      },
      versions: {
        '9.1.0': {
          name: 'pnpm',
          version: '9.1.0',
          dist: {
            shasum: '217063ce3fcbf44f3051666f38b810f1ddefee4a',
            tarball: `${opts.registries.default}pnpm/-/pnpm-9.1.0.tgz`,
            fileCount: 880,
            integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
          },
        },
      },
    })
  nock(opts.registries.default)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, path.join(__dirname, 'pnpm-9.1.0.tgz'))

  await selfUpdate.handler(opts)

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
    .reply(200, {
      name: 'pnpm',
      'dist-tags': {
        latest: '9.0.0',
      },
      versions: {
        '9.0.0': {
          name: 'pnpm',
          version: '9.0.0',
          dist: {
            shasum: '217063ce3fcbf44f3051666f38b810f1ddefee4a',
            tarball: `${opts.registries.default}pnpm/-/pnpm-9.1.0.tgz`,
            fileCount: 880,
            integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
          },
        },
      },
    })

  const output = await selfUpdate.handler(opts)

  expect(output).toBe('The currently active pnpm v9.0.0 is already "latest" and doesn\'t need an update')
})

test('self-update links pnpm that is already present on the disk', async () => {
  const opts = prepare()
  nock(opts.registries.default)
    .get('/pnpm')
    .reply(200, {
      name: 'pnpm',
      'dist-tags': {
        latest: '9.2.0',
      },
      versions: {
        '9.2.0': {
          name: 'pnpm',
          version: '9.2.0',
          dist: {
            shasum: '217063ce3fcbf44f3051666f38b810f1ddefee4a',
            tarball: `${opts.registries.default}pnpm/-/pnpm-9.2.0.tgz`,
            fileCount: 880,
            integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
          },
        },
      },
    })

  const latestPnpmDir = path.join(opts.pnpmHomeDir, '.tools/pnpm/9.2.0/node_modules/pnpm')
  fs.mkdirSync(latestPnpmDir, { recursive: true })
  fs.writeFileSync(path.join(latestPnpmDir, 'package.json'), JSON.stringify({ name: 'pnpm', bin: 'bin.js' }), 'utf8')
  fs.writeFileSync(path.join(latestPnpmDir, 'bin.js'), `#!/usr/bin/env node
console.log('9.2.0')`, 'utf8')
  const output = await selfUpdate.handler(opts)

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
