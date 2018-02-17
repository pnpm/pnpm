import storePath from '@pnpm/store-path'
import {InstallOptions} from 'supi'
import path = require('path')
import createStore from 'package-store'
import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'

const registry = 'http://localhost:4873/'

const retryOpts = {
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 10_000,
  fetchRetryMaxtimeout: 60_000,
}

export default async function testDefaults (
  opts?: any,
  resolveOpts?: any,
  fetchOpts?: any,
  storeOpts?: any,
): Promise<InstallOptions> {
  let store = opts && opts.store || path.resolve('..', '.store')
  store = await storePath(opts && opts.prefix || process.cwd(), store)
  const rawNpmConfig = {registry}
  const storeController = await createStore(
    createResolver({
      metaCache: new Map(),
      rawNpmConfig,
      store,
      strictSsl: true,
      ...retryOpts,
      ...resolveOpts,
    }),
    createFetcher({
      alwaysAuth: true,
      registry,
      rawNpmConfig,
      ...retryOpts,
      ...fetchOpts,
    }) as {},
    {
      store,
      locks: path.join(store, '_locks'),
      ...storeOpts,
    }
  )
  return {
    store,
    storeController,
    registry,
    ...opts,
  }
}
