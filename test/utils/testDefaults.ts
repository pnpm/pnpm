import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import storePath from '@pnpm/store-path'
import createStore, {StoreController} from 'package-store'
import path = require('path')
import {InstallOptions} from 'supi'
import tempy = require('tempy')

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
): Promise<{
  development: boolean,
  optional: boolean,
  production: boolean,
  independentLeaves: boolean,
  storeController: StoreController,
  verifyStoreIntegrity: boolean,
  sideEffectsCache: boolean,
  force: boolean,
  store: string,
}> {
  let store = opts && opts.store || tempy.directory()
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
    development: true,
    optional: true,
    production: true,
    independentLeaves: false,
    verifyStoreIntegrity: true,
    sideEffectsCache: true,
    force: false,
    registry,
    store,
    storeController,
    rawNpmConfig: {},
    unsafePerm: true,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    ...opts,
  }
}
