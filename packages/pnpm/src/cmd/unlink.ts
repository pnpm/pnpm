import { mutateModules } from 'supi'
import createStoreController from '../createStoreController'
import { readImporterManifestFromDir } from '../readImporterManifest'
import { PnpmOptions } from '../types'

export default async function (input: string[], opts: PnpmOptions) {
  const store = await createStoreController(opts)
  const unlinkOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  if (!input || !input.length) {
    return mutateModules([
      {
        dependencyNames: input,
        manifest: await readImporterManifestFromDir(opts.prefix),
        mutation: 'unlinkSome',
        prefix: opts.prefix,
      },
    ], unlinkOpts)
  }
  return mutateModules([
    {
      manifest: await readImporterManifestFromDir(opts.prefix),
      mutation: 'unlink',
      prefix: opts.prefix,
    },
  ], unlinkOpts)
}
