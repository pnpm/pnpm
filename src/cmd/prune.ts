import {PnpmOptions, prune} from 'supi'

export default (input: string[], opts: PnpmOptions) => {
  return prune(opts)
}
