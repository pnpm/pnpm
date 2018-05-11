import path = require('path')
import R = require('ramda')
import {
  link,
  linkFromGlobal,
  linkToGlobal,
} from 'supi'
import createStoreController from '../createStoreController'
import {PnpmOptions} from '../types'

export default async (
  input: string[],
  opts: PnpmOptions,
) => {
  const cwd = opts && opts.prefix || process.cwd()

  const store = await createStoreController(opts)
  const linkOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  // pnpm link
  if (!input || !input.length) {
    await linkToGlobal(cwd, linkOpts)
    return
  }

  const result = R.partition((inp) => inp.startsWith('.'), input)

  const localLinkedPkgs = result[0]
  const globalLinkedPkgs = result[1]

  await link(localLinkedPkgs, path.join(cwd, 'node_modules'), linkOpts)
  await linkFromGlobal(globalLinkedPkgs, cwd, linkOpts)
}
