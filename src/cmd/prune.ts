import {prune} from '../api/prune'
import {PnpmOptions} from '../types'

export default (input: string[], opts: PnpmOptions) => {
  return prune(opts)
}
