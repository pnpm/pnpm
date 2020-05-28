import { Config } from '@pnpm/config'
import createFetcher from '@pnpm/default-fetcher'
import createStore from '@pnpm/package-store'
import fs = require('mz/fs')
import path = require('path')
import createResolver, { CreateResolverOptions } from './createResolver'

export type CreateNewStoreControllerOptions = CreateResolverOptions & Pick<Config,
    | 'alwaysAuth'
    | 'networkConcurrency'
    | 'packageImportMethod'
    | 'registry'
    | 'verifyStoreIntegrity'
  > & {
    ignoreFile?: (filename: string) => boolean,
  }

export default async (
  opts: CreateNewStoreControllerOptions
) => {
  const sopts = Object.assign(opts, {
    registry: opts.registry || 'https://registry.npmjs.org/',
  })
  const resolve = createResolver(sopts)
  await fs.mkdir(sopts.storeDir, { recursive: true })
  const fetchers = createFetcher(sopts)
  return {
    ctrl: await createStore(resolve, fetchers, {
      ignoreFile: sopts.ignoreFile,
      networkConcurrency: sopts.networkConcurrency,
      packageImportMethod: sopts.packageImportMethod,
      storeDir: sopts.storeDir,
      verifyStoreIntegrity: typeof sopts.verifyStoreIntegrity === 'boolean' ?
        sopts.verifyStoreIntegrity : true,
    }),
    dir: sopts.storeDir,
  }
}
