import uninstall from '../api/uninstall'
import {PnpmOptions} from '../types'

export default function uninstallCmd (input: string[], opts: PnpmOptions) {
  return uninstall(input, opts)
}
