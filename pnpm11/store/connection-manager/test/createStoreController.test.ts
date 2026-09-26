import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeEach, expect, jest, test } from '@jest/globals'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'

const getStorePath = jest.fn<(opts: { pkgRoot: string, storePath?: string, pnpmHomeDir: string }) => Promise<string>>()
const globalWarn = jest.fn<(message: string) => void>()

const actualLogger = await import('@pnpm/logger')
const { getStorePathInPnpmHome } = await import('@pnpm/store.path')

jest.unstable_mockModule('@pnpm/logger', () => ({ ...actualLogger, globalWarn }))
jest.unstable_mockModule('@pnpm/store.path', () => ({ getStorePath, getStorePathInPnpmHome }))
jest.unstable_mockModule('@pnpm/installing.client', () => ({
  createClient: jest.fn(() => ({ clearResolutionCache: jest.fn(), fetchers: {}, resolve: jest.fn(), resolutionVerifiers: [] })),
}))
jest.unstable_mockModule('@pnpm/store.controller', () => ({
  createPackageStore: jest.fn(() => ({})),
}))

const { createStoreController } = await import('../src/index.js')
const { closeAllStoreIndexes } = await import('@pnpm/store.index')

const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'create-store-controller-test-'))
const relocatedStoreDir = path.join(tmpDir, 'volume', '.pnpm-store', 'v11')

// Each test uses its own pnpm home directory, as the warning is printed once per home store.
function storeControllerOptions (pnpmHomeDir: string, overrides: Partial<CreateStoreControllerOptions> = {}): CreateStoreControllerOptions {
  return {
    cacheDir: path.join(tmpDir, 'cache'),
    configByUri: {},
    dir: path.join(tmpDir, 'volume', 'project'),
    fetchRetries: 2,
    fetchRetryFactor: 10,
    fetchRetryMaxtimeout: 60_000,
    fetchRetryMintimeout: 10_000,
    offline: false,
    pnpmHomeDir,
    registriesByScope: { default: 'https://registry.npmjs.org/' },
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
    ...overrides,
  } as CreateStoreControllerOptions
}

beforeEach(() => {
  globalWarn.mockClear()
})

afterAll(() => {
  closeAllStoreIndexes()
  fs.rmSync(tmpDir, { recursive: true })
})

test('warns once when the default store is moved away from an existing store in the pnpm home directory', async () => {
  const pnpmHomeDir = path.join(tmpDir, 'home-with-store')
  const homeStoreDir = getStorePathInPnpmHome(pnpmHomeDir)
  fs.mkdirSync(homeStoreDir, { recursive: true })
  getStorePath.mockResolvedValue(relocatedStoreDir)

  await createStoreController(storeControllerOptions(pnpmHomeDir))
  await createStoreController(storeControllerOptions(pnpmHomeDir))

  expect(globalWarn.mock.calls).toStrictEqual([[
    `The store at ${homeStoreDir} is not used because packages cannot be hard linked from it into this project. Using the store at ${relocatedStoreDir} instead. Set storeDir to choose the store.`,
  ]])
})

test('does not warn when the pnpm home directory has no store', async () => {
  getStorePath.mockResolvedValue(relocatedStoreDir)

  await createStoreController(storeControllerOptions(path.join(tmpDir, 'home-without-store')))

  expect(globalWarn).not.toHaveBeenCalled()
})

test('does not warn when the store is configured explicitly', async () => {
  const pnpmHomeDir = path.join(tmpDir, 'home-with-explicit-store')
  fs.mkdirSync(getStorePathInPnpmHome(pnpmHomeDir), { recursive: true })
  getStorePath.mockResolvedValue(relocatedStoreDir)

  await createStoreController(storeControllerOptions(pnpmHomeDir, { storeDir: path.join(tmpDir, 'volume', '.pnpm-store') }))

  expect(globalWarn).not.toHaveBeenCalled()
})

test('does not warn when the store in the pnpm home directory is used', async () => {
  const pnpmHomeDir = path.join(tmpDir, 'home-in-use')
  const homeStoreDir = getStorePathInPnpmHome(pnpmHomeDir)
  fs.mkdirSync(homeStoreDir, { recursive: true })
  getStorePath.mockResolvedValue(homeStoreDir)

  await createStoreController(storeControllerOptions(pnpmHomeDir))

  expect(globalWarn).not.toHaveBeenCalled()
})

test('warns once when store controllers are created concurrently', async () => {
  const pnpmHomeDir = path.join(tmpDir, 'home-concurrent')
  fs.mkdirSync(getStorePathInPnpmHome(pnpmHomeDir), { recursive: true })
  getStorePath.mockResolvedValue(relocatedStoreDir)

  await Promise.all([
    createStoreController(storeControllerOptions(pnpmHomeDir)),
    createStoreController(storeControllerOptions(pnpmHomeDir)),
  ])

  expect(globalWarn).toHaveBeenCalledTimes(1)
})

test('warns once the store in the pnpm home directory appears later in the process', async () => {
  const pnpmHomeDir = path.join(tmpDir, 'home-created-later')
  getStorePath.mockResolvedValue(relocatedStoreDir)

  await createStoreController(storeControllerOptions(pnpmHomeDir))
  expect(globalWarn).not.toHaveBeenCalled()

  fs.mkdirSync(getStorePathInPnpmHome(pnpmHomeDir), { recursive: true })
  await createStoreController(storeControllerOptions(pnpmHomeDir))
  expect(globalWarn).toHaveBeenCalledTimes(1)
})

test('does not warn when the caller skips the bypassed home store warning', async () => {
  const pnpmHomeDir = path.join(tmpDir, 'home-skipped')
  fs.mkdirSync(getStorePathInPnpmHome(pnpmHomeDir), { recursive: true })
  getStorePath.mockResolvedValue(relocatedStoreDir)

  await createStoreController(storeControllerOptions(pnpmHomeDir, { skipBypassedHomeStoreWarning: true }))

  expect(globalWarn).not.toHaveBeenCalled()
})
