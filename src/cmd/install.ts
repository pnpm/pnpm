import {PnpmOptions} from '../types'
import install from '../api/install'

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { quiet: true })
 */
export default function installCmd (input: string[], opts: PnpmOptions) {
  return install(input, opts)
}
