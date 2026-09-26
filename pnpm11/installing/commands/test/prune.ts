import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { add, install, prune } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { symlinkDirSync } from 'symlink-dir'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const f = fixtures(import.meta.dirname)

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  excludeLinksFromLockfile: false,
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
  pnpmfile: ['.pnpmfile.cjs'],
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  configByUri: {},
  registriesByScope: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('prune removes external link that is not in package.json', async () => {
  const project = prepare(undefined)
  const storeDir = path.resolve('store')
  f.copy('local-pkg', 'local')

  symlinkDirSync(path.resolve('local'), path.join('node_modules/local-pkg'))

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

test('prune --prod does not run prepare scripts (pnpm/pnpm#4770)', async () => {
  await using server = await createTestIpcServer()

  const project = prepare({
    name: 'test-prune-prod-skips-prepare',
    version: '0.0.0',
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
    scripts: {
      preinstall: server.sendLineScript('preinstall'),
      install: server.sendLineScript('install'),
      postinstall: server.sendLineScript('postinstall'),
      preprepare: server.sendLineScript('preprepare'),
      prepare: `node -e "require('is-negative')" && ${server.sendLineScript('prepare')}`,
      postprepare: server.sendLineScript('postprepare'),
    },
  })
  const opts = {
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir: path.resolve('store'),
  }

  await install.handler(opts)
  expect(server.getLines()).toStrictEqual(['preinstall', 'install', 'postinstall', 'preprepare', 'prepare', 'postprepare'])
  server.clear()

  await prune.handler({ ...opts, dev: false })

  project.has('is-positive')
  project.hasNot('is-negative')
  expect(server.getLines()).toStrictEqual(['preinstall', 'install', 'postinstall'])
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
