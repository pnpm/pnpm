import {
  rebuild,
  rebuildPkgs,
} from 'supi'
import createStoreController from '../createStoreController'
import { PnpmOptions } from '../types'

export default async function (
  args: string[],
  opts: PnpmOptions,
  command: string,
) {
  const store = await createStoreController(opts)
  const rebuildOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  if (args.length === 0) {
    await rebuild([{ buildIndex: 0, prefix: rebuildOpts.prefix }], rebuildOpts)
  }
  await rebuildPkgs([{ prefix: rebuildOpts.prefix }], args, rebuildOpts)
}
