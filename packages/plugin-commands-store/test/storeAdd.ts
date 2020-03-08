import { store } from '@pnpm/plugin-commands-store'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fs = require('fs')
import loadJsonFile = require('load-json-file')
import path = require('path')
import exists = require('path-exists')
import test = require('tape')

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

  const pathToCheck = path.join(storeDir, '2', `localhost+${REGISTRY_MOCK_PORT}`, 'express', '4.16.3')
  t.ok(await exists(pathToCheck), `express@4.16.3 is in store (at ${pathToCheck})`)

  const storeIndex = await loadJsonFile(path.join(storeDir, '2', 'store.json'))
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

  const pathToCheck = path.join(storeDir, '2', `localhost+${REGISTRY_MOCK_PORT}`, '@foo', 'no-deps', '1.0.0')
  t.ok(await exists(pathToCheck), `@foo/no-deps@1.0.0 is in store (at ${pathToCheck})`)

  const storeIndex = await loadJsonFile(path.join(storeDir, '2', 'store.json'))
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
