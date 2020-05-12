import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import createStore from '@pnpm/package-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { StoreController } from '@pnpm/store-controller-types'
import storePath from '@pnpm/store-path'
import { Registries } from '@pnpm/types'
import path = require('path')
import { InstallOptions } from 'supi'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

const retryOpts = {
  fetchRetries: 4,
  fetchRetryFactor: 10,
  fetchRetryMaxtimeout: 60_000,
  fetchRetryMintimeout: 10_000,
}

export default async function testDefaults<T> (
  opts?: T & {
    fastUnpack?: boolean,
    storeDir?: string,
    prefix?: string,
  }, // tslint:disable-line
  resolveOpts?: any, // tslint:disable-line
  fetchOpts?: any, // tslint:disable-line
  storeOpts?: any, // tslint:disable-line
): Promise<
  InstallOptions &
  {
    registries: Registries,
    storeController: StoreController,
    storeDir: string,
  } &
  T
> {
  let storeDir = opts && opts.storeDir || path.resolve('.store')
  storeDir = await storePath(opts && opts.prefix || process.cwd(), storeDir)
  const rawConfig = { registry }
  const storeController = await createStore(
    createResolver({
      fullMetadata: false,
      metaCache: new Map(),
      rawConfig,
      storeDir,
      strictSsl: true,
      ...retryOpts,
      ...resolveOpts,
    }),
    createFetcher({
      alwaysAuth: true,
      rawConfig,
      registry,
      ...retryOpts,
      ...fetchOpts,
    }) as {},
    {
      ignoreFile: opts?.fastUnpack === false ? undefined : (filename) => filename !== 'package.json',
      locks: path.join(storeDir, '_locks'),
      storeDir,
      verifyStoreIntegrity: true,
      ...storeOpts,
    }
  )
  return {
    registries: {
      default: registry,
    },
    storeController,
    storeDir,
    ...opts,
  } as (
    InstallOptions &
    {
      registries: Registries,
      storeController: StoreController,
      storeDir: string,
    } &
    T
  )
}
