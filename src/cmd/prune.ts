import {prune, prunePkgs} from '../api/prune'
import {PublicInstallationOptions} from '../api/install'

export default (input: string[], opts: PublicInstallationOptions) => {
  if (input && input.length) {
    return prunePkgs(input, opts)
  }
  return prune(opts)
}
