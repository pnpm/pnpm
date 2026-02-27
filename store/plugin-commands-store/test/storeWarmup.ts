import fs from 'fs'
import path from 'path'
import { STORE_VERSION } from '@pnpm/constants'
import { store } from '@pnpm/plugin-commands-store'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import execa from 'execa'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

test('pnpm store warmup populates GVS without creating node_modules', async () => {
  prepareEmpty()
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  // First: do a normal install to generate a lockfile
  await execa('node', [
    pnpmBin,
    'add',
    'is-positive@1.0.0',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    `--registry=${REGISTRY}`,
  ])

  // Verify lockfile exists
  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  // Remove node_modules and the store's links dir (GVS)
  rimraf('node_modules')
  const linksDir = path.join(storeDir, STORE_VERSION, 'links')
  rimraf(linksDir)

  // Run store warmup
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: { registry: REGISTRY },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['warmup'])

  // GVS links dir should be populated
  expect(fs.existsSync(linksDir)).toBeTruthy()

  // node_modules should NOT be created
  expect(fs.existsSync('node_modules')).toBeFalsy()
})

test('pnpm store warmup fails without lockfile', async () => {
  prepareEmpty()
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  await expect(
    store.handler({
      cacheDir,
      dir: process.cwd(),
      lockfileDir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: { registry: REGISTRY },
      registries: { default: REGISTRY },
      storeDir,
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['warmup'])
  ).rejects.toThrow('Cannot warm up the store')
})

test('pnpm store warmup is idempotent', async () => {
  prepareEmpty()
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  // Generate lockfile
  await execa('node', [
    pnpmBin,
    'add',
    'is-positive@1.0.0',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    `--registry=${REGISTRY}`,
  ])

  rimraf('node_modules')

  const warmupOpts = {
    cacheDir,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: { registry: REGISTRY },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }

  // Run warmup twice — second should succeed without errors
  await store.handler(warmupOpts, ['warmup'])
  await store.handler(warmupOpts, ['warmup'])

  const linksDir = path.join(storeDir, STORE_VERSION, 'links')
  expect(fs.existsSync(linksDir)).toBeTruthy()
  expect(fs.existsSync('node_modules')).toBeFalsy()
})
