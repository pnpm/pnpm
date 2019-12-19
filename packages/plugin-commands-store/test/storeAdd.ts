import { store } from '@pnpm/plugin-commands-store'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import loadJsonFile = require('load-json-file')
import path = require('path')
import exists = require('path-exists')
import test = require('tape')

test('pnpm store add express@4.16.3', async function (t) {
  tempDir(t)

  const storeDir = path.resolve('store')

  await store.handler(['add', 'express@4.16.3'], {
    dir: process.cwd(),
    lock: true,
    rawConfig: {
      registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
    storeDir,
  })

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

  await store.handler(['add', '@foo/no-deps@1.0.0'], {
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
  })

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
