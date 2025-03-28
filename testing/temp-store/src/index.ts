import * as path from 'path'
import { type ClientOptions, createClient } from '@pnpm/client'
import { createPackageStore, type CreatePackageStoreOptions } from '@pnpm/package-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { type StoreController } from '@pnpm/store-controller-types'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

export interface CreateTempStoreResult {
  storeController: StoreController
  storeDir: string
  cacheDir: string
}

export function createTempStore (opts?: {
  fastUnpack?: boolean
  storeDir?: string
  clientOptions?: ClientOptions
  storeOptions?: CreatePackageStoreOptions
}): CreateTempStoreResult {
  const authConfig = { registry }
  const cacheDir = path.resolve('cache')
  const { resolve, fetchers, clearResolutionCache } = createClient({
    authConfig,
    rawConfig: {},
    retry: {
      retries: 4,
      factor: 10,
      maxTimeout: 60_000,
      minTimeout: 10_000,
    },
    cacheDir,
    ...opts?.clientOptions,
  })
  const storeDir = opts?.storeDir ?? path.resolve('.store')
  const storeController = createPackageStore(
    resolve,
    fetchers,
    {
      cacheDir,
      ignoreFile: opts?.fastUnpack === false ? undefined : (filename) => filename !== 'package.json',
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
      clearResolutionCache,
      ...opts?.storeOptions,
    }
  )
  return {
    storeController,
    storeDir,
    cacheDir,
  }
}
