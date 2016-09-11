import * as link from '../api/link'
import {PublicInstallationOptions} from '../api/install'

export default (input: string[], opts: PublicInstallationOptions) => {
  if (!input || !input.length) {
    return link.linkToGlobal(opts)
  }
  if (input[0].indexOf('.') === 0) {
    return link.linkFromRelative(input[0], opts)
  }
  return link.linkFromGlobal(input[0], opts)
}
