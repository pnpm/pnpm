import {PnpmOptions, unlink, unlinkPkgs} from 'supi'

export default function (input: string[], opts: PnpmOptions) {
  if (!input || !input.length) {
    return unlink(opts)
  }
  return unlinkPkgs(input, opts)
}
