import { Config } from '@pnpm/config'
import createFetcher from '@pnpm/default-fetcher'
import logger from '@pnpm/logger'
import createStore from '@pnpm/package-store'
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import makeDir = require('make-dir')
import path = require('path')
import createResolver from './createResolver'

export default async (
  opts: Pick<Config,
    'alwaysAuth' |
    'ca' |
    'cert' |
    'fetchRetries' |
    'fetchRetryFactor' |
    'fetchRetryMaxtimeout' |
    'fetchRetryMintimeout' |
    'fetchRetryMintimeout' |
    'httpsProxy' |
    'key' |
    'localAddress' |
    'lock' |
    'lockStaleDuration' |
    'networkConcurrency' |
    'offline' |
    'packageImportMethod' |
    'proxy' |
    'rawConfig' |
    'registry' |
    'strictSsl' |
    'userAgent' |
    'verifyStoreIntegrity'
  > & {
    ignoreFile?: (filename: string) => boolean,
  } & Required<Pick<Config, 'store'>>,
) => {
  // TODO: either print a warning or just log if --no-lock is used
  const sopts = Object.assign(opts, {
    locks: opts.lock ? path.join(opts.store, '_locks') : undefined,
    registry: opts.registry || 'https://registry.npmjs.org/',
  })
  const resolve = createResolver(sopts)
  await makeDir(sopts.store)
  const fsIsCaseSensitive = await dirIsCaseSensitive(sopts.store)
  logger.debug({
    // An undefined field would cause a crash of the logger
    // so converting it to null
    isCaseSensitive: typeof fsIsCaseSensitive === 'boolean'
      ? fsIsCaseSensitive : null,
    store: sopts.store,
  })
  const fetchers = createFetcher({ ...sopts, fsIsCaseSensitive })
  return {
    ctrl: await createStore(resolve, fetchers as {}, {
      locks: sopts.locks,
      lockStaleDuration: sopts.lockStaleDuration,
      networkConcurrency: sopts.networkConcurrency,
      packageImportMethod: sopts.packageImportMethod,
      store: sopts.store,
      verifyStoreIntegrity: typeof sopts.verifyStoreIntegrity === 'boolean' ?
        sopts.verifyStoreIntegrity : true,
    }),
    path: sopts.store,
  }
}
