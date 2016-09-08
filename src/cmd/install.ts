import install from '../api/install'

/*
 * Perform installation.
 *
 *     installCmd([ 'lodash', 'foo' ], { quiet: true })
 */

export default function installCmd (input, opts) {
  return install(input, opts)
}
