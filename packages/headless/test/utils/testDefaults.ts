import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import storePath from '@pnpm/store-path'
import createStore, {StoreController} from 'package-store'
import path = require('path')
import tempy = require('tempy')
import {HeadlessOptions} from '@pnpm/headless'

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
): Promise<HeadlessOptions> {
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
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    independentLeaves: false,
    verifyStoreIntegrity: true,
    sideEffectsCache: true,
    force: false,
    registries: {
      default: registry,
    },
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
