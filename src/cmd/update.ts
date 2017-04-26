import {PnpmOptions} from '../types'
import {install, installPkgs} from '../api/install'

export default function (input: string[], opts: PnpmOptions) {
  opts = Object.assign({update: true}, opts)

  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}
