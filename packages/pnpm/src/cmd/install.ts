import {
  install,
  installPkgs,
} from 'supi'
import createStoreController from '../createStoreController'
import requireHooks from '../requireHooks'
import {PnpmOptions} from '../types'

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { silent: true })
 */
export default async function installCmd (
  input: string[],
  opts: PnpmOptions,
) {
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const prefix = opts.prefix || process.cwd()
  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(prefix, opts)
  }

  const store = await createStoreController(opts)
  const installOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  if (!input || !input.length) {
    return install(installOpts)
  }
  return installPkgs(input, installOpts)
}
