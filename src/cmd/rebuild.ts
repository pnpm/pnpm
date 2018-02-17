import storePath from '@pnpm/store-path'
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
  const rebuildOpts = Object.assign(opts, {
    store: await storePath(opts.prefix, opts.store),
  })

  if (args.length === 0) {
    await rebuild(rebuildOpts)
  }
  await rebuildPkgs(args, rebuildOpts)
}
