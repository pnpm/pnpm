import {uninstall, PnpmOptions} from 'supi'

export default function uninstallCmd (input: string[], opts: PnpmOptions) {
  return uninstall(input, opts)
}
