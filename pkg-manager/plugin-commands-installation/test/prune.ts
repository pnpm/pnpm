import path from 'path'
import { add, install, link, prune } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import fs from 'fs'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const f = fixtures(__dirname)

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  linkWorkspacePackages: true,
  pnpmfile: '.pnpmfile.cjs',
  pnpmHomeDir: '',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
}

test('prune removes external link that is not in package.json', async () => {
  const project = prepare(undefined)
  const storeDir = path.resolve('store')
  f.copy('local-pkg', 'local')

  await link.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
  }, ['./local'])

  project.has('local-pkg')

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
  })

  project.hasNot('local-pkg')
})

test('prune keeps hoisted dependencies', async () => {
  const project = prepare(undefined)
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  await add.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: process.cwd(),
    storeDir,
  }, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'])

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: process.cwd(),
    storeDir,
  })

  project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
})

test('prune removes dev dependencies', async () => {
  const project = prepare({
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
  })
  const storeDir = path.resolve('store')

  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dev: false,
    dir: process.cwd(),
    storeDir,
  })

  project.has('is-positive')
  project.has('.pnpm/is-positive@1.0.0')
  project.hasNot('is-negative')
  project.hasNot('.pnpm/is-negative@1.0.0')
})

test('prune: ignores all the lifecycle scripts when --ignore-scripts is used', async () => {
  await using server = await createTestIpcServer()

  prepare({
    name: 'test-prune-with-ignore-scripts',
    version: '0.0.0',

    scripts: {
      // eslint-disable:object-literal-sort-keys
      preinstall: server.sendLineScript('preinstall'),
      prepare: server.sendLineScript('prepare'),
      postinstall: server.sendLineScript('postinstall'),
      // eslint-enable:object-literal-sort-keys
    },
  })

  const storeDir = path.resolve('store')

  const opts = {
    ...DEFAULT_OPTIONS,
    ignoreScripts: true,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  }

  await install.handler(opts)

  await prune.handler(opts)

  expect(fs.existsSync('package.json')).toBeTruthy()
  expect(server.getLines()).toStrictEqual([])
})

test('cliOptionsTypes', () => {
  expect(prune.cliOptionsTypes()).toHaveProperty('production')
  expect(prune.cliOptionsTypes()).toHaveProperty('dev')
  expect(prune.cliOptionsTypes()).toHaveProperty('ignore-scripts')
  expect(prune.cliOptionsTypes()).toHaveProperty('optional')
})
