import * as path from 'path'
import createClient from '@pnpm/client'
import createStore from '@pnpm/package-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { StoreController } from '@pnpm/store-controller-types'
import storePath from '@pnpm/store-path'
import { Registries } from '@pnpm/types'
import { InstallOptions } from '@pnpm/core'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

const retryOpts = {
  retries: 4,
  retryFactor: 10,
  retryMaxtimeout: 60_000,
  retryMintimeout: 10_000,
}

export default async function testDefaults<T> (
  opts?: T & {
    fastUnpack?: boolean
    storeDir?: string
    prefix?: string
  }, // eslint-disable-line
  resolveOpts?: any, // eslint-disable-line
  fetchOpts?: any, // eslint-disable-line
  storeOpts?: any // eslint-disable-line
): Promise<
  InstallOptions &
  {
    cacheDir: string
    registries: Registries
    storeController: StoreController
    storeDir: string
  } &
  T
  > {
  const authConfig = { registry }
  const cacheDir = path.resolve('cache')
  const { resolve, fetchers } = createClient({
    authConfig,
    retry: retryOpts,
    cacheDir,
    ...resolveOpts,
    ...fetchOpts,
  })
  let storeDir = opts?.storeDir ?? path.resolve('.store')
  storeDir = await storePath(opts?.prefix ?? process.cwd(), storeDir)
  const storeController = await createStore(
    resolve,
    fetchers,
    {
      ignoreFile: opts?.fastUnpack === false ? undefined : (filename) => filename !== 'package.json',
      storeDir,
      verifyStoreIntegrity: true,
      ...storeOpts,
    }
  )
  const result = {
    cacheDir,
    registries: {
      default: registry,
    },
    storeController,
    storeDir,
    ...opts,
  } as (
    InstallOptions &
    {
      cacheDir: string
      registries: Registries
      storeController: StoreController
      storeDir: string
    } &
    T
  )
  return result
}
