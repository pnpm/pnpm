import fs from 'fs'
import path from 'path'
import { prependDirsToPath } from '@pnpm/env.path'
import { tempDir } from '@pnpm/prepare'
import { selfUpdate } from '@pnpm/tools.plugin-commands-self-updater'
import spawn from 'cross-spawn'
import nock from 'nock'

const registry = 'https://registry.npmjs.org/'

jest.mock('@pnpm/cli-meta', () => {
  const actualModule = jest.requireActual('@pnpm/cli-meta')

  return {
    ...actualModule,
    packageManager: {
      name: actualModule.packageManager.name,
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

test('self-update', async () => {
  const dir = tempDir(false)
  nock(registry)
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
            tarball: 'https://registry.npmjs.org/pnpm/-/pnpm-9.1.0.tgz',
            fileCount: 880,
            integrity: 'sha512-Z/WHmRapKT5c8FnCOFPVcb6vT3U8cH9AyyK+1fsVeMaq07bEEHzLO6CzW+AD62IaFkcayDbIe+tT+dVLtGEnJA==',
          },
        },
      },
    })
  nock(registry)
    .get('/pnpm/-/pnpm-9.1.0.tgz')
    .replyWithFile(200, path.join(__dirname, 'pnpm-9.1.0.tgz'))

  await selfUpdate.handler({
    argv: {
      original: [],
    },
    cliOptions: {},
    linkWorkspacePackages: true,
    bail: true,
    pnpmHomeDir: dir,
    registries: {
      default: registry,
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
  })

  const pnpmPkgJson = JSON.parse(fs.readFileSync(path.join(dir, '.tools/pnpm/9.1.0/node_modules/pnpm/package.json'), 'utf8'))
  expect(pnpmPkgJson.version).toBe('9.1.0')

  const pnpmEnv = prependDirsToPath([dir])
  const { status, stdout } = spawn.sync('pnpm', ['-v'], {
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
    },
  })
  expect(status).toBe(0)
  expect(stdout.toString().trim()).toBe('9.1.0')
})
