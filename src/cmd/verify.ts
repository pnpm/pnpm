import verify from '../api/verify'
import {PnpmOptions} from '../types'
import {PnpmError} from '../errorTypes'

export default async function (input: string[], opts: PnpmOptions) {
  const modifiedPkgs = await verify(opts)
  if (!modifiedPkgs || !modifiedPkgs.length) return

  throw new VerificationError(modifiedPkgs)
}

class VerificationError extends PnpmError {
  constructor (modified: string[]) {
    super('MODIFIED_DEPENDENCY', '')
    this.modified = modified
  }
  modified: string[]
}
