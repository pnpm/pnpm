import {install, installPkgs, PnpmOptions} from 'supi'

export default function (input: string[], opts: PnpmOptions) {
  opts = Object.assign({update: true}, opts)

  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}
