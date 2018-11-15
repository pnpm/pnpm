import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import { HeadlessOptions } from '@pnpm/headless'
import createStore from '@pnpm/package-store'
import readManifests from '@pnpm/read-manifests'
import storePath from '@pnpm/store-path'
import path = require('path')
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
): Promise<HeadlessOptions> {
  let store = opts && opts.store || tempy.directory()
  const shrinkwrapDirectory = opts && opts.shrinkwrapDirectory || process.cwd()
  const manifests = await readManifests(
    [
      {
        prefix: shrinkwrapDirectory,
      },
    ],
    shrinkwrapDirectory,
    {
      shamefullyFlatten: opts.shamefullyFlatten,
    },
  )
  store = await storePath(shrinkwrapDirectory, store)
  const rawNpmConfig = { registry }
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
    force: false,
    importers: manifests.importers,
    include: manifests.include,
    independentLeaves: false,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    pendingBuilds: manifests.pendingBuilds,
    rawNpmConfig: {},
    registries: manifests.registries || {
      default: registry,
    },
    shrinkwrapDirectory,
    sideEffectsCache: true,
    store,
    storeController,
    unsafePerm: true,
    verifyStoreIntegrity: true,
    ...opts,
  }
}
