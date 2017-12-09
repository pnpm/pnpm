import {install, installPkgs, PnpmOptions} from 'supi'
import createStoreController from '../createStoreController'
import requireHooks from '../requireHooks'

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { silent: true })
 */
export default async function installCmd (input: string[], opts: PnpmOptions) {
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const prefix = opts.prefix || process.cwd()
  opts.hooks = requireHooks(prefix)

  opts['storeController'] = await createStoreController(opts) // tslint:disable-line

  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}
