import * as link from '../api/link'
import {PnpmOptions} from '../types'

export default (input: string[], opts: PnpmOptions) => {
  if (!input || !input.length) {
    return link.linkToGlobal(opts)
  }
  if (input[0].indexOf('.') === 0) {
    return link.linkFromRelative(input[0], opts)
  }
  return link.linkFromGlobal(input[0], opts)
}
