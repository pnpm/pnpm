import {install, installPkgs, PnpmOptions} from 'supi'
import requireHooks from '../requireHooks'

export default function (input: string[], opts: PnpmOptions) {
  opts = Object.assign({update: true}, opts)

  const prefix = opts.prefix || process.cwd()
  opts.hooks = requireHooks(prefix)

  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}
