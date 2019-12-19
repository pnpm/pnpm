import { store } from '@pnpm/plugin-commands-store'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import test = require('tape')

test('pnpm store usages CLI does not fail', async function (t) {
  tempDir(t)

  // Call store usages
  await store.handler(['usages', 'is-odd@2.0.0', '@babel/core', 'ansi-regex'], {
    dir: process.cwd(),
    lock: true,
    rawConfig: {
      registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registries: { default: `http://localhost:${REGISTRY_MOCK_PORT}/` },
    storeDir: './pnpm-store',
  })
  t.pass('CLI did not fail')
  t.end()
})
