import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import createStore from '@pnpm/package-store'
import { StoreController } from '@pnpm/store-controller-types'
import storePath from '@pnpm/store-path'
import { Registries } from '@pnpm/types'
import path = require('path')
import { InstallOptions } from 'supi'

const registry = 'http://localhost:4873/'

const retryOpts = {
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMaxtimeout: 60_000,
  fetchRetryMintimeout: 10_000,
}

export default async function testDefaults<T> (
  opts?: T & {
    store?: string,
    prefix?: string,
  }, // tslint:disable-line
  resolveOpts?: any, // tslint:disable-line
  fetchOpts?: any, // tslint:disable-line
  storeOpts?: any, // tslint:disable-line
): Promise<
  InstallOptions &
  {
    registries: Registries,
    store: string,
    storeController: StoreController,
  } &
  T
> {
  let store = opts && opts.store || path.resolve('.store')
  store = await storePath(opts && opts.prefix || process.cwd(), store)
  const rawNpmConfig = { registry }
  const storeController = await createStore(
    createResolver({
      fullMetadata: false,
      metaCache: new Map(),
      rawNpmConfig,
      store,
      strictSsl: true,
      ...retryOpts,
      ...resolveOpts,
    }),
    createFetcher({
      alwaysAuth: true,
      rawNpmConfig,
      registry,
      ...retryOpts,
      ...fetchOpts,
    }) as {},
    {
      locks: path.join(store, '_locks'),
      store,
      verifyStoreIntegrity: true,
      ...storeOpts,
    },
  )
  return {
    registries: {
      default: registry,
    },
    store,
    storeController,
    ...opts,
  } as (
    InstallOptions &
    {
      registries: Registries,
      store: string,
      storeController: StoreController,
    } &
    T
  )
}
