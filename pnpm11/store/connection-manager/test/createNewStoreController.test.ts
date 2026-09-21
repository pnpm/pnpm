import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, jest, test } from '@jest/globals'
import type { ClientOptions } from '@pnpm/installing.client'
import { closeAllStoreIndexes } from '@pnpm/store.index'

const createClient = jest.fn<(opts: ClientOptions) => ReturnType<typeof import('@pnpm/installing.client').createClient>>()

jest.unstable_mockModule('@pnpm/installing.client', () => ({ createClient }))
jest.unstable_mockModule('@pnpm/store.controller', () => ({
  createPackageStore: jest.fn(() => ({})),
}))

const { createNewStoreController } = await import('../src/createNewStoreController.js')
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'store-controller-test-'))
const requiredOptions = {
  configByUri: {},
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMaxtimeout: 60_000,
  fetchRetryMintimeout: 10_000,
  offline: false,
  registriesByScope: { default: 'https://registry.npmjs.org/' },
  verifyStoreIntegrity: true,
  virtualStoreDirMaxLength: 120,
}
const client = {
  clearResolutionCache: jest.fn(),
  fetchers: {},
  resolve: jest.fn(),
  resolutionVerifiers: [],
} as unknown as ReturnType<typeof import('@pnpm/installing.client').createClient>

afterAll(() => {
  closeAllStoreIndexes()
  fs.rmSync(tmpDir, { recursive: true })
})

test('reads cafile and passes its certificates to the client', async () => {
  const cafile = path.join(tmpDir, 'custom-ca.pem')
  fs.writeFileSync(cafile, 'first\n-----END CERTIFICATE-----\nsecond\n-----END CERTIFICATE-----\n')
  createClient.mockReturnValue(client)

  await createNewStoreController({
    ...requiredOptions,
    cacheDir: path.join(tmpDir, 'cache'),
    storeDir: path.join(tmpDir, 'store'),
    cafile,
  })

  expect(createClient).toHaveBeenCalledWith(expect.objectContaining({
    ca: [
      'first\n-----END CERTIFICATE-----',
      'second\n-----END CERTIFICATE-----',
    ],
  }))
})

test('explicit ca takes precedence over cafile', async () => {
  createClient.mockReturnValue(client)

  await createNewStoreController({
    ...requiredOptions,
    cacheDir: path.join(tmpDir, 'explicit-cache'),
    storeDir: path.join(tmpDir, 'explicit-store'),
    ca: 'explicit ca',
    cafile: '/path/that/must/not/be/read',
  })

  expect(createClient).toHaveBeenLastCalledWith(expect.objectContaining({ ca: 'explicit ca' }))
})
