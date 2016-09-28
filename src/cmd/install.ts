import {PnpmOptions} from '../types'
import {install, installPkgs} from '../api/install'

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { quiet: true })
 */
export default function installCmd (input: string[], opts: PnpmOptions) {
  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}
