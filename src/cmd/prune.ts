import {prune, PnpmOptions} from 'supi'

export default (input: string[], opts: PnpmOptions) => {
  return prune(opts)
}
