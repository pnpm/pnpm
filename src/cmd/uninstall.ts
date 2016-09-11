import uninstall from '../api/uninstall'
import {PublicInstallationOptions} from '../api/install'

export default function uninstallCmd (input: string[], opts: PublicInstallationOptions) {
  return uninstall(input, opts)
}
