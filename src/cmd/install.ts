import {install, installPkgs, PnpmOptions} from 'supi'

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { silent: true })
 */
export default function installCmd (input: string[], opts: PnpmOptions) {
  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}
