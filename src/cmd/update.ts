import {PnpmOptions} from '../types'
import {install, installPkgs} from '../api/install'

export default function (input: string[], opts: PnpmOptions) {
  opts = Object.assign({depth: 0, cacheTTL: 0}, opts)

  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}
