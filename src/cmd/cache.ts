import {PnpmOptions} from '../types'
import {cleanCache} from '../api/cache'

export default function (input: string[], opts: PnpmOptions) {
  if (input.length !== 1 || input[0] !== 'clean') {
    throw new Error('Currently only the `cache clean` command is supported')
  }
  return cleanCache(opts.cachePath)
}
