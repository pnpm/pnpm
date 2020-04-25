import { Config } from '@pnpm/config'
import createFetcher from '@pnpm/default-fetcher'
import logger from '@pnpm/logger'
import createStore from '@pnpm/package-store'
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import fs = require('mz/fs')
import path = require('path')
import createResolver, { CreateResolverOptions } from './createResolver'

export type CreateNewStoreControllerOptions = CreateResolverOptions & Pick<Config,
    'alwaysAuth' |
    'lock' |
    'lockStaleDuration' |
    'networkConcurrency' |
    'packageImportMethod' |
    'registry' |
    'verifyStoreIntegrity'
  > & {
    ignoreFile?: (filename: string) => boolean,
  }

export default async (
  opts: CreateNewStoreControllerOptions,
) => {
  // TODO: either print a warning or just log if --no-lock is used
  const sopts = Object.assign(opts, {
    locks: opts.lock ? path.join(opts.storeDir, '_locks') : undefined,
    registry: opts.registry || 'https://registry.npmjs.org/',
  })
  const resolve = createResolver(sopts)
  await fs.mkdir(sopts.storeDir, { recursive: true })
  const fsIsCaseSensitive = await dirIsCaseSensitive(sopts.storeDir)
  logger.debug({
    // An undefined field would cause a crash of the logger
    // so converting it to null
    isCaseSensitive: typeof fsIsCaseSensitive === 'boolean'
      ? fsIsCaseSensitive : null,
    store: sopts.storeDir,
  })
  const fetchers = createFetcher({ ...sopts, fsIsCaseSensitive })
  return {
    ctrl: await createStore(resolve, fetchers as {}, {
      locks: sopts.locks,
      lockStaleDuration: sopts.lockStaleDuration,
      networkConcurrency: sopts.networkConcurrency,
      packageImportMethod: sopts.packageImportMethod,
      storeDir: sopts.storeDir,
      verifyStoreIntegrity: typeof sopts.verifyStoreIntegrity === 'boolean' ?
        sopts.verifyStoreIntegrity : true,
    }),
    dir: sopts.storeDir,
  }
}
