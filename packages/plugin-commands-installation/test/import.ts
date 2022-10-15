/// <reference path="../../../typings/index.d.ts" />
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { PnpmError } from '@pnpm/error'
import { importCommand } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import tempy from 'tempy'

const f = fixtures(__dirname)

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`
const TMP = tempy.directory()

const DEFAULT_OPTS = {
  alwaysAuth: false,
  ca: undefined,
  cacheDir: path.join(TMP, 'cache'),
  cert: undefined,
  fetchRetries: 2,
  fetchRetryFactor: 90,
  fetchRetryMaxtimeout: 90,
  fetchRetryMintimeout: 10,
  httpsProxy: undefined,
  key: undefined,
  localAddress: undefined,
  lock: false,
  lockStaleDuration: 90,
  networkConcurrency: 16,
  offline: false,
  proxy: undefined,
  pnpmHomeDir: '',
  rawConfig: { registry: REGISTRY },
  registries: { default: REGISTRY },
  registry: REGISTRY,
  storeDir: path.join(TMP, 'store'),
  strictSsl: false,
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
}

test('import from package-lock.json', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  f.prepare('has-package-lock-json')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])

  // node_modules is not created
  await project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await project.hasNot('@pnpm.e2e/pkg-with-1-dep')
})

test('import from yarn.lock', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  f.prepare('has-yarn-lock')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])

  // node_modules is not created
  await project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await project.hasNot('@pnpm.e2e/pkg-with-1-dep')
})

test('import from yarn2 lock file', async () => {
  f.prepare('has-yarn2-lock')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/is-positive/1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/is-negative/1.0.0'])

  // node_modules is not created
  await project.hasNot('balanced-match')
  await project.hasNot('brace-expansion')
})

test('import from npm-shrinkwrap.json', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  f.prepare('has-npm-shrinkwrap-json')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])

  // node_modules is not created
  await project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  await project.hasNot('@pnpm.e2e/pkg-with-1-dep')
})

test('import fails when no lockfiles are found', async () => {
  prepare(undefined)

  await expect(
    importCommand.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('LOCKFILE_NOT_FOUND', 'No lockfile found')
  )
})
