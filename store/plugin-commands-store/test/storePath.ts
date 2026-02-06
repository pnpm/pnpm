import os from 'os'
import path from 'path'
import { STORE_VERSION } from '@pnpm/constants'
import { store } from '@pnpm/plugin-commands-store'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('CLI prints the current store path', async () => {
  prepare()

  const candidateStorePath = await store.handler({
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir: '/home/example/.pnpm-store',
    userConfig: {},
    dlxCacheMaxAge: 0,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['path'])

  const expectedStorePath = os.platform() === 'win32'
    ? `\\home\\example\\.pnpm-store\\${STORE_VERSION}`
    : `/home/example/.pnpm-store/${STORE_VERSION}`

  expect(candidateStorePath).toBe(expectedStorePath)
})

test('CLI prints the current store path when storeDir is relative', async () => {
  prepare()

  const workspaceDir = process.cwd()
  const subpackageDir = path.join(workspaceDir, 'packages', 'foo')
  const relativeStoreDir = '../store'

  const candidateStorePath = await store.handler({
    cacheDir: path.resolve('cache'),
    dir: subpackageDir,
    workspaceDir,
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir: relativeStoreDir,
    userConfig: {},
    dlxCacheMaxAge: 0,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['path'])

  // relativeStoreDir should resolve from workspaceDir, not dir
  const expectedStorePath = path.join(workspaceDir, relativeStoreDir, STORE_VERSION)
  expect(candidateStorePath).toBe(expectedStorePath)
})
