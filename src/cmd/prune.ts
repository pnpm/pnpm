import {PnpmOptions, prune} from 'supi'
import createStoreController from '../createStoreController'

export default async (input: string[], opts: PnpmOptions) => {
  opts['storeController'] = await createStoreController(opts) // tslint:disable-line
  return prune(opts)
}
