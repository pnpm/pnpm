import fs from 'fs'
import path from 'path'
import assertStore from '@pnpm/assert-store'
import { store } from '@pnpm/plugin-commands-store'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const STORE_VERSION = 'v3'

test('pnpm store add express@4.16.3', async () => {
  tempDir()

  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
    storeDir,
    userConfig: {},
  }, ['add', 'express@4.16.3'])

  const { cafsHas } = assertStore(path.join(storeDir, STORE_VERSION))
  await cafsHas('sha1-avilAjUNsyRuzEvs9rWjTSL37VM=')
})

test('pnpm store add scoped package that uses not the standard registry', async () => {
  tempDir()

  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: 'https://registry.npmjs.org/',
    },
    registries: {
      '@foo': `http://localhost:${REGISTRY_MOCK_PORT}/`,
      default: 'https://registry.npmjs.org/',
    },
    storeDir,
    userConfig: {},
  }, ['add', '@foo/no-deps@1.0.0'])

  const { cafsHas } = assertStore(path.join(storeDir, STORE_VERSION))
  await cafsHas('@foo/no-deps', '1.0.0')
})

test('should fail if some packages can not be added', async () => {
  tempDir()
  fs.mkdirSync('_')
  process.chdir('_')
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('pnpm-store')

  let thrown = false
  try {
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: 'https://registry.npmjs.org/',
      },
      registries: {
        '@foo': `http://localhost:${REGISTRY_MOCK_PORT}/`,
        default: 'https://registry.npmjs.org/',
      },
      storeDir,
      userConfig: {},
    }, ['add', '@pnpm/this-does-not-exist'])
  } catch (e: any) { // eslint-disable-line
    thrown = true
    expect(e.code).toBe('ERR_PNPM_STORE_ADD_FAILURE')
    expect(e.message).toBe('Some packages have not been added correctly')
  }
  expect(thrown).toBeTruthy()
})
