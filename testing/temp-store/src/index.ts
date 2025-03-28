import * as path from 'path'
import { createClient } from '@pnpm/client'
import { createPackageStore } from '@pnpm/package-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { type StoreController } from '@pnpm/store-controller-types'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

const DEFAULT_RETRY_OPTS = {
  retries: 4,
  retryFactor: 10,
  retryMaxtimeout: 60_000,
  retryMintimeout: 10_000,
}

export function createTempStore (opts?: {
  fastUnpack?: boolean
  storeDir?: string
  clientOptions?: any // eslint-disable-line
  storeOptions?: any // eslint-disable-line
  retry?: any // eslint-disable-line
}): {
    storeController: StoreController
    storeDir: string
    cacheDir: string
  } {
  const authConfig = { registry }
  const cacheDir = path.resolve('cache')
  const { resolve, fetchers, clearResolutionCache } = createClient({
    authConfig,
    rawConfig: {},
    retry: {
      ...DEFAULT_RETRY_OPTS,
      ...opts?.retry,
    },
    cacheDir,
    ...opts?.clientOptions,
  })
  const storeDir = opts?.storeDir ?? path.resolve('.store')
  const storeController = createPackageStore(
    resolve,
    fetchers,
    {
      ignoreFile: opts?.fastUnpack === false ? undefined : (filename) => filename !== 'package.json',
      storeDir,
      verifyStoreIntegrity: true,
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
