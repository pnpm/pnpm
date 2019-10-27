import {
  rebuild,
  rebuildPkgs,
} from 'supi'
import createStoreController from '../createStoreController'
import { readImporterManifestOnly } from '../readImporterManifest'
import { PnpmOptions } from '../types'

export default async function (
  args: string[],
  opts: PnpmOptions,
  command: string,
) {
  const store = await createStoreController(opts)
  const rebuildOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  if (args.length === 0) {
    await rebuild(
      [
        {
          buildIndex: 0,
          manifest: await readImporterManifestOnly(rebuildOpts.dir, opts),
          prefix: rebuildOpts.dir,
        },
      ],
      rebuildOpts,
    )
  }
  await rebuildPkgs(
    [
      {
        manifest: await readImporterManifestOnly(rebuildOpts.dir, opts),
        prefix: rebuildOpts.dir,
      },
    ],
    args,
    rebuildOpts,
  )
}
