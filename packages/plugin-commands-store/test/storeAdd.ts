import { getFilePathInCafs } from '@pnpm/cafs'
import { store } from '@pnpm/plugin-commands-store'
import { tempDir } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fs = require('fs')
import loadJsonFile = require('load-json-file')
import path = require('path')
import exists = require('path-exists')
import test = require('tape')

const STORE_VERSION = 'v3'

test('pnpm store add express@4.16.3', async function (t) {
  tempDir(t)

  const storeDir = path.resolve('store')

  await store.handler({
    dir: process.cwd(),
    lock: true,
    rawConfig: {
      registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
    storeDir,
  }, ['add', 'express@4.16.3'])

  const pathToCheck = path.join(storeDir, STORE_VERSION, 'files/6a/f8a502350db3246ecc4becf6b5a34d22f7ed53.json')
  t.ok(await exists(pathToCheck), `express@4.16.3 is in store (at ${pathToCheck})`)

  const storeIndex = await loadJsonFile(path.join(storeDir, STORE_VERSION, 'store.json'))
  t.deepEqual(
    storeIndex,
    {
      [`localhost+${REGISTRY_MOCK_PORT}/express/4.16.3`]: [],
    },
    'package has been added to the store index',
  )
  t.end()
})

test('pnpm store add scoped package that uses not the standard registry', async function (t) {
  tempDir(t)

  const storeDir = path.resolve('store')

  await store.handler({
    dir: process.cwd(),
    lock: true,
    rawConfig: {
      registry: 'https://registry.npmjs.org/',
    },
    registries: {
      '@foo': `http://localhost:${REGISTRY_MOCK_PORT}/`,
      'default': 'https://registry.npmjs.org/',
    },
    storeDir,
  }, ['add', '@foo/no-deps@1.0.0'])

  const cafsDir = path.join(storeDir, STORE_VERSION, 'files')
  const pathToCheck = getFilePathInCafs(cafsDir, {
    integrity: getIntegrity('@foo/no-deps', '1.0.0'),
    mode: 0,
  }) + '.json'
  t.ok(await exists(pathToCheck), `@foo/no-deps@1.0.0 is in store (at ${pathToCheck})`)

  const storeIndex = await loadJsonFile(path.join(storeDir, STORE_VERSION, 'store.json'))
  t.deepEqual(
    storeIndex,
    {
      [`localhost+${REGISTRY_MOCK_PORT}/@foo/no-deps/1.0.0`]: [],
    },
    'package has been added to the store index',
  )
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
      lock: true,
      rawConfig: {
        registry: 'https://registry.npmjs.org/',
      },
      registries: {
        '@foo': `http://localhost:${REGISTRY_MOCK_PORT}/`,
        'default': 'https://registry.npmjs.org/',
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
