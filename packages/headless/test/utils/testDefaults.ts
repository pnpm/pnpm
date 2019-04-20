import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import { HeadlessOptions } from '@pnpm/headless'
import createStore from '@pnpm/package-store'
import readImportersContext from '@pnpm/read-importers-context'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
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
  const lockfileDirectory = opts && opts.lockfileDirectory || process.cwd()
  const { importers, include, pendingBuilds, registries } = await readImportersContext(
    [
      {
        prefix: lockfileDirectory,
      },
    ],
    lockfileDirectory,
    {
      shamefullyFlatten: opts.shamefullyFlatten,
    },
  )
  store = await storePath(lockfileDirectory, store)
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
    currentEngine: {
      nodeVersion: process.version,
      pnpmVersion: '2.0.0',
    },
    engineStrict: false,
    force: false,
    importers: opts.importers ? opts.importers : await Promise.all(
      importers.map(async (importer) => ({ ...importer, manifest: await readPackageJsonFromDir(importer.prefix) }))
    ),
    include,
    independentLeaves: false,
    lockfileDirectory,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    pendingBuilds,
    rawNpmConfig: {},
    registries: registries || {
      default: registry,
    },
    sideEffectsCache: true,
    skipped: new Set<string>(),
    store,
    storeController,
    unsafePerm: true,
    verifyStoreIntegrity: true,
    ...opts,
  }
}
