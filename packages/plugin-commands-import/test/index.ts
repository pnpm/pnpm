/// <reference path="../../../typings/index.d.ts" />
import { promisify } from 'util'
import assertProject from '@pnpm/assert-project'
import PnpmError from '@pnpm/error'
import { importCommand } from '@pnpm/plugin-commands-import'
import prepare, { tempDir } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import path = require('path')
import ncpCB = require('ncp')
import test = require('tape')
import tempy = require('tempy')

const ncp = promisify(ncpCB)

const fixtures = path.join(__dirname, '../../../fixtures')

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTS = {
  alwaysAuth: false,
  ca: undefined,
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
  storeDir: tempy.directory(),
  strictSsl: false,
  userAgent: 'pnpm',
  useRunningStoreServer: false,
  useStoreServer: false,
}

test('import from package-lock.json', async (t) => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  tempDir(t)

  await ncp(path.join(fixtures, 'has-package-lock-json'), process.cwd())

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const project = assertProject(t, process.cwd())
  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  // node_modules is not created
  await project.hasNot('dep-of-pkg-with-1-dep')
  await project.hasNot('pkg-with-1-dep')

  t.end()
})

test('import from npm-shrinkwrap.json', async (t) => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  tempDir(t)

  await ncp(path.join(fixtures, 'has-npm-shrinkwrap-json'), process.cwd())

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const project = assertProject(t, process.cwd())
  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  // node_modules is not created
  await project.hasNot('dep-of-pkg-with-1-dep')
  await project.hasNot('pkg-with-1-dep')

  t.end()
})

test('import fails when no npm lockfiles are found', async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    await importCommand.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err.message.toString().includes('No package-lock.json or npm-shrinkwrap.json found'))

  t.end()
})
