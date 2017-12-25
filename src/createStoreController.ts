import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import loadJsonFile = require('load-json-file')
import path = require('path')
import {PnpmOptions} from 'supi'
import extendOptions from 'supi/lib/api/extendOptions'
import createStore from './createStore'

export default async function (opts: PnpmOptions) {
  const strictOpts = await extendOptions(opts, false) // TODO: Only the store path is needed here
  try {
    const serverJson = await loadJsonFile(path.join(strictOpts.store, 'server.json'))
    logger.info('A store service is running and will be used to download the needed packages')
    return {
      ctrl: await connectStoreController(serverJson.connectionOptions), // tslint:disable-line
      path: strictOpts.store,
    }
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return await createStore(strictOpts)
}
