import fs from 'fs'
import path from 'path'
import { assertStore } from '@pnpm/assert-store'
import { Lockfile } from '@pnpm/lockfile-file'
import { store } from '@pnpm/plugin-commands-store'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import isEmpty from 'ramda/src/isEmpty'
import ssri from 'ssri'

const STORE_VERSION = 'v3'
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.cjs')

test('remove unreferenced packages', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    'is-negative@^2.1.0',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    `--registry=${REGISTRY}`])
  await execa('node', [
    pnpmBin,
    'remove',
    'is-negative',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--config.modules-cache-max-age=0',
  ], { env: { npm_config_registry: REGISTRY } })

  await project.storeHas('is-negative', '2.1.0')

  const reporter = jest.fn()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
  }, ['prune'])

  expect(reporter).toBeCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 1 package',
    })
  )

  await project.storeHasNot('is-negative', '2.1.0')

  reporter.mockClear()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
  }, ['prune'])

  expect(reporter).not.toBeCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 1 package',
    })
  )
  expect(fs.readdirSync(cacheDir).length).toEqual(0)
})

test.skip('remove packages that are used by project that no longer exist', async () => {
  prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store', STORE_VERSION)
  const { cafsHas, cafsHasNot } = assertStore(storeDir)

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  await rimraf('node_modules')

  await cafsHas(ssri.fromHex('f0d86377aa15a64c34961f38ac2a9be2b40a1187', 'sha1').toString())

  const reporter = jest.fn()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
  }, ['prune'])

  expect(reporter).toBeCalledWith(
    expect.objectContaining({
      level: 'info',
      message: `- localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    })
  )

  await cafsHasNot(ssri.fromHex('f0d86377aa15a64c34961f38ac2a9be2b40a1187', 'sha1').toString())
})

test('keep dependencies used by others', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'camelcase-keys@3.0.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'add', 'hastscript@3.0.0', '--save-dev', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'camelcase-keys', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  await project.storeHas('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHas('camelcase', '3.0.0')

  await project.storeHas('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  // all dependencies are marked as dev
  const lockfile = await project.readLockfile() as Lockfile
  expect(isEmpty(lockfile.packages)).toBeFalsy()

  Object.entries(lockfile.packages ?? {}).forEach(([depPath, dep]) => expect(dep.dev).toBeTruthy())

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
  }, ['prune'])

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.storeHasNot('map-obj', '1.0.1')
  await project.storeHas('camelcase', '3.0.0')
})

test('keep dependency used by package', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'is-not-positive@1.0.0', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'is-not-positive', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
  }, ['prune'])

  await project.storeHas('is-positive', '3.1.0')
})

test('prune will skip scanning non-directory in storeDir', async () => {
  prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'is-not-positive@1.0.0', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])
  fs.writeFileSync(path.join(storeDir, STORE_VERSION, 'files/.DS_store'), 'foobar')

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
  }, ['prune'])
})

test('prune does not fail if the store contains an unexpected directory', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  await project.storeHas('is-negative', '2.1.0')
  const alienDir = path.join(storeDir, 'v3/files/44/directory')
  fs.mkdirSync(alienDir)

  const reporter = jest.fn()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
  }, ['prune'])

  expect(reporter).toBeCalledWith(
    expect.objectContaining({
      level: 'warn',
      message: `An alien directory is present in the store: ${alienDir}`,
    })
  )
})
