import {prune, prunePkgs} from '../api/prune'
import {PnpmOptions} from '../types'

export default (input: string[], opts: PnpmOptions) => {
  if (input && input.length) {
    return prunePkgs(input, opts)
  }
  return prune(opts)
}
