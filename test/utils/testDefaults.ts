import {InstallOptions} from 'supi'
import path = require('path')
import createStore, {resolveStore} from 'package-store'
import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'

const registry = 'http://localhost:4873/'

export default async function testDefaults (
  opts?: any,
  resolveOpts?: any,
  fetchOpts?: any,
  storeOpts?: any,
): Promise<InstallOptions> {
  let store = opts && opts.store || path.resolve('..', '.store')
  store = await resolveStore(store, opts && opts.prefix || process.cwd())
  const rawNpmConfig = {registry}
  const storeController = await createStore(
    createResolver({
      metaCache: new Map(),
      rawNpmConfig,
      store,
      strictSsl: true,
      ...resolveOpts,
    }),
    createFetcher({
      alwaysAuth: true,
      registry,
      rawNpmConfig,
      ...fetchOpts,
    }) as {},
    {
      store,
      locks: path.join(store, '_locks'),
      ...storeOpts,
    }
  )
  return Object.assign({
    store,
    storeController,
    registry,
  }, opts)
}
