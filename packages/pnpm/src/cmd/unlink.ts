import { mutateModules } from 'supi'
import createStoreController from '../createStoreController'
import { readImporterManifestOnly } from '../readImporterManifest'
import { PnpmOptions } from '../types'

export default async function (input: string[], opts: PnpmOptions) {
  const store = await createStoreController(opts)
  const unlinkOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  if (!input || !input.length) {
    return mutateModules([
      {
        dependencyNames: input,
        manifest: await readImporterManifestOnly(opts.dir, opts),
        mutation: 'unlinkSome',
        prefix: opts.dir,
      },
    ], unlinkOpts)
  }
  return mutateModules([
    {
      manifest: await readImporterManifestOnly(opts.dir, opts),
      mutation: 'unlink',
      prefix: opts.dir,
    },
  ], unlinkOpts)
}
