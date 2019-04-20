import { InstallOptions, mutateModules } from 'supi'
import createStoreController from '../createStoreController'
import { readImporterManifestFromDir } from '../readImporterManifest'
import { PnpmOptions } from '../types'

export default async (input: string[], opts: PnpmOptions) => {
  const store = await createStoreController(opts)
  return mutateModules([
    {
      buildIndex: 0,
      manifest: await readImporterManifestFromDir(process.cwd()),
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
