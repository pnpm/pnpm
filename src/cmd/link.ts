import path = require('path')
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

  await input.reduce((previous: Promise<void>, inp: string) => {
    // pnpm link ../foo
    if (inp[0].indexOf('.') === 0) {
      const linkFrom = path.join(cwd, inp)
      return previous.then(() => link(linkFrom, cwd, linkOpts))
    }

    // pnpm link foo
    return previous.then(() => linkFromGlobal(inp, cwd, linkOpts))
  }, Promise.resolve())
}
