import os from 'os'
import path from 'path'
import { store } from '@pnpm/plugin-commands-store'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('CLI prints the current store path', async () => {
  prepare()

  const candidateStorePath = await store.handler({
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir: '/home/example/.pnpm-store',
    userConfig: {},
  }, ['path'])

  const expectedStorePath = os.platform() === 'win32'
    ? '\\home\\example\\.pnpm-store\\v3'
    : '/home/example/.pnpm-store/v3'

  expect(candidateStorePath).toBe(expectedStorePath)
})
