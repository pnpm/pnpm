import {resolveStore} from 'package-store'
import path = require('path')
import {
  rebuild,
  rebuildPkgs,
} from 'supi'
import {PnpmOptions} from '../types'

export default async function (
  args: string[],
  opts: PnpmOptions,
  command: string,
) {
  const rebuildOpts = Object.assign(opts, {store: await resolveStore(opts.store, opts.prefix)})

  if (args.length === 0) {
    await rebuild(rebuildOpts)
  }
  await rebuildPkgs(args, rebuildOpts)
}
