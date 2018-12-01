import { install, InstallOptions } from 'supi'
import createStoreController from '../createStoreController'
import { PnpmOptions } from '../types'

export default async (input: string[], opts: PnpmOptions) => {
  const store = await createStoreController(opts)
  return install({
    ...opts,
    importers: [
      {
        operation: 'install',
        prefix: process.cwd(),
        pruneDirectDependencies: true,
      },
    ],
    pruneStore: true,
    store: store.path,
    storeController: store.ctrl,
  } as InstallOptions)
}
