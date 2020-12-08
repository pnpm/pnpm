import assertStore from '@pnpm/assert-store'
import { store } from '@pnpm/plugin-commands-store'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fs = require('fs')
import path = require('path')

const STORE_VERSION = 'v3'

test('pnpm store add express@4.16.3', async () => {
  tempDir(undefined)

  const storeDir = path.resolve('store')

  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
    storeDir,
  }, ['add', 'express@4.16.3'])

  const { cafsHas } = assertStore(undefined, path.join(storeDir, STORE_VERSION))
  await cafsHas('sha1-avilAjUNsyRuzEvs9rWjTSL37VM=')
})

test('pnpm store add scoped package that uses not the standard registry', async () => {
  tempDir(undefined)

  const storeDir = path.resolve('store')

  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: 'https://registry.npmjs.org/',
    },
    registries: {
      '@foo': `http://localhost:${REGISTRY_MOCK_PORT}/`,
      default: 'https://registry.npmjs.org/',
    },
    storeDir,
  }, ['add', '@foo/no-deps@1.0.0'])

  const { cafsHas } = assertStore(undefined, path.join(storeDir, STORE_VERSION))
  await cafsHas('@foo/no-deps', '1.0.0')
})

test('should fail if some packages can not be added', async () => {
  tempDir(undefined)
  fs.mkdirSync('_')
  process.chdir('_')
  const storeDir = path.resolve('pnpm-store')

  let thrown = false
  try {
    await store.handler({
      dir: process.cwd(),
      rawConfig: {
        registry: 'https://registry.npmjs.org/',
      },
      registries: {
        '@foo': `http://localhost:${REGISTRY_MOCK_PORT}/`,
        default: 'https://registry.npmjs.org/',
      },
      storeDir,
    }, ['add', '@pnpm/this-does-not-exist'])
  } catch (e) {
    thrown = true
    expect(e.code).toBe('ERR_PNPM_STORE_ADD_FAILURE')
    expect(e.message).toBe('Some packages have not been added correctly')
  }
  expect(thrown).toBeTruthy()
})
