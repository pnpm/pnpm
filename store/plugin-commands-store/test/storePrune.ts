import fs from 'fs'
import path from 'path'
import { assertStore } from '@pnpm/assert-store'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { store } from '@pnpm/plugin-commands-store'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import execa from 'execa'
import ssri from 'ssri'

const STORE_VERSION = 'v3'
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

const createCacheKey = (...pkgs: string[]): string => dlx.createCacheKey(pkgs, { default: REGISTRY })

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

  project.storeHas('is-negative', '2.1.0')

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
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 1 package',
    })
  )

  project.storeHasNot('is-negative', '2.1.0')

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
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])

  expect(reporter).not.toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 1 package',
    })
  )
  expect(fs.readdirSync(cacheDir)).toStrictEqual([])
})

test.skip('remove packages that are used by project that no longer exist', async () => {
  prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store', STORE_VERSION)
  const { cafsHas, cafsHasNot } = assertStore(storeDir)

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  rimraf('node_modules')

  cafsHas(ssri.fromHex('f0d86377aa15a64c34961f38ac2a9be2b40a1187', 'sha1').toString())

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
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'info',
      message: `- localhost+${REGISTRY_MOCK_PORT}/is-negative/2.1.0`,
    })
  )

  cafsHasNot(ssri.fromHex('f0d86377aa15a64c34961f38ac2a9be2b40a1187', 'sha1').toString())
})

test('keep dependencies used by others', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'camelcase-keys@3.0.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'add', 'hastscript@3.0.0', '--save-dev', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'camelcase-keys', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  project.storeHas('camelcase-keys', '3.0.0')
  project.hasNot('camelcase-keys')

  project.storeHas('camelcase', '3.0.0')

  project.storeHas('map-obj', '1.0.1')
  project.hasNot('map-obj')

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
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])

  project.storeHasNot('camelcase-keys', '3.0.0')
  project.storeHasNot('map-obj', '1.0.1')
  project.storeHas('camelcase', '3.0.0')
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
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])

  project.storeHas('is-positive', '3.1.0')
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
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])
})

test('prune does not fail if the store contains an unexpected directory', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  project.storeHas('is-negative', '2.1.0')
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
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'warn',
      message: `An alien directory is present in the store: ${alienDir}`,
    })
  )

  // as force is not used, the alien directory is not removed
  expect(fs.existsSync(alienDir)).toBeTruthy()
})

test('prune removes alien files from the store if the --force flag is used', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  project.storeHas('is-negative', '2.1.0')
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
    force: true,
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])
  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'warn',
      message: `An alien directory has been removed from the store: ${alienDir}`,
    })
  )
  expect(fs.existsSync(alienDir)).toBeFalsy()
})

describe('prune when store directory is not properly configured', () => {
  test('prune will not fail if the store directory does not exist (ENOENT)', async () => {
    prepareEmpty()
    const nonExistentStoreDir = path.resolve('store')
    const reporter = jest.fn()

    await expect(
      store.handler({
        cacheDir: path.resolve('cache'),
        dir: process.cwd(),
        pnpmHomeDir: '',
        rawConfig: {
          registry: REGISTRY,
        },
        registries: { default: REGISTRY },
        reporter,
        storeDir: nonExistentStoreDir,
        userConfig: {},
        dlxCacheMaxAge: Infinity,
        virtualStoreDirMaxLength: 120,
      }, ['prune'])
    ).resolves.toBeUndefined()

    expect(reporter).toHaveBeenCalledWith(
      expect.objectContaining({
        level: 'info',
        message: 'Removed 0 files',
      })
    )

    expect(reporter).toHaveBeenCalledWith(
      expect.objectContaining({
        level: 'info',
        message: 'Removed 0 packages',
      })
    )
  })

  test('prune will fail for other file-related errors (i.e.; not ENOENT)', async () => {
    prepareEmpty()
    const fileInPlaceOfStoreDir = path.resolve('store')
    fs.writeFileSync(fileInPlaceOfStoreDir, '')
    await expect(
      store.handler({
        cacheDir: path.resolve('cache'),
        dir: process.cwd(),
        pnpmHomeDir: '',
        rawConfig: {
          registry: REGISTRY,
        },
        registries: { default: REGISTRY },
        reporter: jest.fn(),
        storeDir: fileInPlaceOfStoreDir,
        userConfig: {},
        dlxCacheMaxAge: Infinity,
        virtualStoreDirMaxLength: 120,
      }, ['prune'])
    ).rejects.toThrow(/^ENOTDIR/)
  })
})

function createSampleDlxCacheLinkTarget (dirPath: string): void {
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.pnpm'), { recursive: true })
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.bin'), { recursive: true })
  fs.writeFileSync(path.join(dirPath, 'node_modules', '.modules.yaml'), '')
  fs.writeFileSync(path.join(dirPath, 'package.json'), '')
  fs.writeFileSync(path.join(dirPath, 'pnpm-lock.yaml'), '')
}

function createSampleDlxCacheItem (cacheDir: string, cmd: string, now: Date, age: number): void {
  const hash = createCacheKey(cmd)
  const newDate = new Date(now.getTime() - age * 60_000)
  const timeError = 432 // just an arbitrary amount, nothing is special about this number
  const pid = 71014 // just an arbitrary number to represent pid
  const targetName = `${(newDate.getTime() - timeError).toString(16)}-${pid.toString(16)}`
  const linkTarget = path.join(cacheDir, 'dlx', hash, targetName)
  const linkPath = path.join(cacheDir, 'dlx', hash, 'pkg')
  createSampleDlxCacheLinkTarget(linkTarget)
  fs.symlinkSync(linkTarget, linkPath, 'junction')
  fs.lutimesSync(linkPath, newDate, newDate)
}

function createSampleDlxCacheFsTree (cacheDir: string, now: Date, ageTable: Record<string, number>): void {
  for (const [cmd, age] of Object.entries(ageTable)) {
    createSampleDlxCacheItem(cacheDir, cmd, now, age)
  }
}

test('prune removes cache directories that outlives dlx-cache-max-age', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  fs.mkdirSync(path.join(storeDir, 'v3', 'files'), { recursive: true })
  fs.mkdirSync(path.join(storeDir, 'v3', 'tmp'), { recursive: true })

  const now = new Date()

  createSampleDlxCacheFsTree(cacheDir, now, {
    foo: 1,
    bar: 5,
    baz: 20,
  })

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter () {},
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: 7,
    virtualStoreDirMaxLength: 120,
  }, ['prune'])

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual(
    ['foo', 'bar']
      .map(cmd => createCacheKey(cmd))
      .sort()
  )
})
