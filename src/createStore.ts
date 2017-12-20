import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import createStore from 'package-store'
import { PnpmOptions } from 'supi'
import extendOptions from 'supi/lib/api/extendOptions'

export default async (opts: PnpmOptions) => {
  const strictOpts = await extendOptions(opts)

  const resolve = createResolver(strictOpts)
  const fetchers = createFetcher(strictOpts)
  return {
    ctrl: await createStore(resolve, fetchers as {}, {
      lockStaleDuration: strictOpts.lockStaleDuration,
      locks: strictOpts.locks,
      networkConcurrency: strictOpts.networkConcurrency,
      store: strictOpts.store,
    }),
    path: strictOpts.store,
  }
}
