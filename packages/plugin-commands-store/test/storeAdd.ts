import assertStore from '@pnpm/assert-store'
import { store } from '@pnpm/plugin-commands-store'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fs = require('fs')
import path = require('path')
import test = require('tape')

const STORE_VERSION = 'v3'

test('pnpm store add express@4.16.3', async function (t) {
  tempDir(t)

  const storeDir = path.resolve('store')

  await store.handler({
    dir: process.cwd(),
    rawConfig: {
      registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
    storeDir,
  }, ['add', 'express@4.16.3'])

  const { cafsHas } = assertStore(t, path.join(storeDir, STORE_VERSION))
  await cafsHas('sha1-avilAjUNsyRuzEvs9rWjTSL37VM=')

  t.end()
})

test('pnpm store add scoped package that uses not the standard registry', async function (t) {
  tempDir(t)

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

  const { cafsHas } = assertStore(t, path.join(storeDir, STORE_VERSION))
  await cafsHas('@foo/no-deps', '1.0.0')

  t.end()
})

test('should fail if some packages can not be added', async (t) => {
  tempDir(t)
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
    t.equal(e.code, 'ERR_PNPM_STORE_ADD_FAILURE', 'has thrown the correct error code')
    t.equal(e.message, 'Some packages have not been added correctly', 'has thrown the correct error')
  }
  t.ok(thrown, 'has thrown')
  t.end()
})
