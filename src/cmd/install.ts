import install, {PublicInstallationOptions} from '../api/install'

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { quiet: true })
 */
export default function installCmd (input: string[], opts: PublicInstallationOptions) {
  return install(input, opts)
}
