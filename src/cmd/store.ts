import storeStatus from '../api/storeStatus'
import {PnpmOptions} from '../types'
import {PnpmError} from '../errorTypes'

export default async function (input: string[], opts: PnpmOptions) {
  if (input[0] !== 'status') {
    throw new Error('Unknown command')
  }
  const modifiedPkgs = await storeStatus(opts)
  if (!modifiedPkgs || !modifiedPkgs.length) return

  throw new StoreStatusError(modifiedPkgs)
}

class StoreStatusError extends PnpmError {
  constructor (modified: string[]) {
    super('MODIFIED_DEPENDENCY', '')
    this.modified = modified
  }
  modified: string[]
}
