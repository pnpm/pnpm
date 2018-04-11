import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import storePath from '@pnpm/store-path'
import createStore from 'package-store'
import path = require('path')
import {InstallOptions} from 'supi'

const registry = 'http://localhost:4873/'

const retryOpts = {
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMaxtimeout: 60_000,
  fetchRetryMintimeout: 10_000,
}

export default async function testDefaults (
  opts?: any, // tslint:disable-line
  resolveOpts?: any, // tslint:disable-line
  fetchOpts?: any, // tslint:disable-line
  storeOpts?: any, // tslint:disable-line
): Promise<InstallOptions & {globalPrefix: string, globalBin: string}> {
  let store = opts && opts.store || path.resolve('..', '.store')
  store = await storePath(opts && opts.prefix || process.cwd(), store)
  const rawNpmConfig = {registry}
  const storeController = await createStore(
    createResolver({
      fullMetadata: true, // temporarily. Till the lifecycle hooks performance issue is solved
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
      ...storeOpts,
    },
  )
  return {
    registry,
    store,
    storeController,
    ...opts,
  }
}
