/// <reference path="../../../typings/index.d.ts" />
import { promisify } from 'util'
import path from 'path'
import assertProject from '@pnpm/assert-project'
import PnpmError from '@pnpm/error'
import { importCommand } from '@pnpm/plugin-commands-import'
import prepare, { tempDir } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import tempy from 'tempy'
import ncpCB from 'ncp'

const ncp = promisify(ncpCB)

const fixtures = path.join(__dirname, '../../../fixtures')

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
  rawConfig: { registry: REGISTRY },
  registries: { default: REGISTRY },
  registry: REGISTRY,
  storeDir: path.join(TMP, 'store'),
  strictSsl: false,
  userAgent: 'pnpm',
  useRunningStoreServer: false,
  useStoreServer: false,
}

test('import from package-lock.json', async () => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  tempDir()

  await ncp(path.join(fixtures, 'has-package-lock-json'), process.cwd())

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/dep-of-pkg-with-1-dep/100.1.0'])

  // node_modules is not created
  await project.hasNot('dep-of-pkg-with-1-dep')
  await project.hasNot('pkg-with-1-dep')
})

test('import from yarn.lock', async () => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  tempDir()

  await ncp(path.join(fixtures, 'has-yarn-lock'), process.cwd())

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.1.0'])
  expect(lockfile.packages).not.toHaveProperty(['/dep-of-pkg-with-1-dep/100.0.0'])

  // node_modules is not created
  await project.hasNot('dep-of-pkg-with-1-dep')
  await project.hasNot('pkg-with-1-dep')
})

test('import from npm-shrinkwrap.json', async () => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  tempDir()

  await ncp(path.join(fixtures, 'has-npm-shrinkwrap-json'), process.cwd())

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/dep-of-pkg-with-1-dep/100.1.0'])

  // node_modules is not created
  await project.hasNot('dep-of-pkg-with-1-dep')
  await project.hasNot('pkg-with-1-dep')
})

test('import fails when no lockfiles are found', async () => {
  prepare(undefined)

  await expect(
    importCommand.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  ).rejects.toThrow(
    new PnpmError('LOCKFILE_NOT_FOUND', 'No lockfile found')
  )
})
