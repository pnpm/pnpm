import {
  install,
  installPkgs,
} from 'supi'
import createStoreController from '../createStoreController'
import requireHooks from '../requireHooks'
import {PnpmOptions} from '../types'

export default async function (
  input: string[],
  opts: PnpmOptions,
) {
  const store = await createStoreController(opts)
  const updateOpts = Object.assign(opts, {
    allowNew: false,
    hooks: !opts.ignorePnpmfile && requireHooks(opts.prefix, opts),
    store: store.path,
    storeController: store.ctrl,
    update: true,
  })

  if (!input || !input.length) {
    return install(updateOpts)
  }
  return installPkgs(input, updateOpts)
}
