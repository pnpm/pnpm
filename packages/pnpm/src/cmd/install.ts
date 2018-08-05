import {StoreController} from 'package-store'
import {
  install,
  InstallOptions,
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
  opts: PnpmOptions & {
    store?: string,
    storeController?: StoreController,
  },
) {
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const prefix = opts.prefix || process.cwd()
  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(prefix, opts)
  }

  const installOpts = (
    opts.storeController && opts.store
      ? opts
      : await (async () => {
          const store = await createStoreController(opts)
          return Object.assign(opts, {
            store: store.path,
            storeController: store.ctrl,
          })
        })()
  ) as InstallOptions

  if (!input || !input.length) {
    return install(installOpts)
  }
  return installPkgs(input, installOpts)
}
