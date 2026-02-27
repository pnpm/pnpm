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

function defaultOpts (storeDir: string, cacheDir: string) {
  return {
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
}

test('pnpm store warmup populates GVS without creating node_modules', async () => {
  prepareEmpty()
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  // Install to generate a lockfile and populate the CAS
  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    `--registry=${REGISTRY}`,
  ])

  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()

  // Remove node_modules and the store's links dir (GVS)
  rimraf('node_modules')
  const linksDir = path.join(storeDir, STORE_VERSION, 'links')
  rimraf(linksDir)

  // Run store warmup
  await store.handler(defaultOpts(storeDir, cacheDir), ['warmup'])

  // GVS links dir should be populated with actual package directories
  expect(fs.existsSync(linksDir)).toBeTruthy()
  const scopeDir = path.join(linksDir, '@pnpm.e2e')
  expect(fs.existsSync(scopeDir)).toBeTruthy()

  // Verify the scoped package has version directories with hash subdirs
  const pkgVersionDir = path.join(scopeDir, 'pkg-with-1-dep', '100.0.0')
  expect(fs.existsSync(pkgVersionDir)).toBeTruthy()
  const hashDirs = fs.readdirSync(pkgVersionDir)
  expect(hashDirs.length).toBeGreaterThan(0)

  // Verify internal node_modules with actual package files
  const gvsPackageDir = path.join(pkgVersionDir, hashDirs[0], 'node_modules', '@pnpm.e2e', 'pkg-with-1-dep')
  expect(fs.existsSync(path.join(gvsPackageDir, 'package.json'))).toBeTruthy()

  // Verify transitive dep was also imported
  const transitiveDep = path.join(pkgVersionDir, hashDirs[0], 'node_modules', '@pnpm.e2e', 'dep-of-pkg-with-1-dep')
  expect(fs.existsSync(path.join(transitiveDep, 'package.json'))).toBeTruthy()

  // node_modules should NOT be created
  expect(fs.existsSync('node_modules')).toBeFalsy()
})

test('pnpm store warmup fails without lockfile', async () => {
  prepareEmpty()
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  await expect(
    store.handler(defaultOpts(storeDir, cacheDir), ['warmup'])
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_NO_LOCKFILE',
  })
})

test('pnpm store warmup is idempotent', async () => {
  prepareEmpty()
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  await execa('node', [
    pnpmBin,
    'add',
    'is-positive@1.0.0',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    `--registry=${REGISTRY}`,
  ])

  rimraf('node_modules')

  const opts = defaultOpts(storeDir, cacheDir)

  // Run warmup twice — second should succeed without errors
  await store.handler(opts, ['warmup'])
  await store.handler(opts, ['warmup'])

  const linksDir = path.join(storeDir, STORE_VERSION, 'links')
  expect(fs.existsSync(linksDir)).toBeTruthy()
  expect(fs.existsSync('node_modules')).toBeFalsy()
})

test('pnpm store warmup handles scoped packages and transitive deps', async () => {
  prepareEmpty()
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  // Install multiple packages including scoped ones with transitive deps
  await execa('node', [
    pnpmBin,
    'add',
    '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    'is-positive@1.0.0',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    `--registry=${REGISTRY}`,
  ])

  rimraf('node_modules')
  const linksDir = path.join(storeDir, STORE_VERSION, 'links')
  rimraf(linksDir)

  await store.handler(defaultOpts(storeDir, cacheDir), ['warmup'])

  // Both scoped and unscoped packages should exist in the GVS
  expect(fs.existsSync(path.join(linksDir, '@pnpm.e2e'))).toBeTruthy()

  // Verify internal symlinks were created (transitive dep accessible from parent)
  const pkgVersionDir = path.join(linksDir, '@pnpm.e2e', 'pkg-with-1-dep', '100.0.0')
  const hashDirs = fs.readdirSync(pkgVersionDir)
  const internalNodeModules = path.join(pkgVersionDir, hashDirs[0], 'node_modules')
  expect(fs.existsSync(internalNodeModules)).toBeTruthy()

  expect(fs.existsSync('node_modules')).toBeFalsy()
})
