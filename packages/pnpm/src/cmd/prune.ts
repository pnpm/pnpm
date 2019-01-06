import { InstallOptions, mutateModules } from 'supi'
import createStoreController from '../createStoreController'
import { PnpmOptions } from '../types'

export default async (input: string[], opts: PnpmOptions) => {
  const store = await createStoreController(opts)
  return mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      prefix: process.cwd(),
      pruneDirectDependencies: true,
    },
  ], {
    ...opts,
    pruneStore: true,
    store: store.path,
    storeController: store.ctrl,
  } as InstallOptions)
}
