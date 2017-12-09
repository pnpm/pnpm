import logger from '@pnpm/logger'
import {connectStoreController} from '@pnpm/server'
import loadJsonFile = require('load-json-file')
import path = require('path')
import {PnpmOptions} from 'supi'
import extendOptions from 'supi/lib/api/extendOptions'

export default async function (opts: PnpmOptions) {
  try {
    const strictOpts = await extendOptions(opts) // TODO: Only the store path is needed here
    const serverJson = await loadJsonFile(path.join(strictOpts.store, 'server.json'))
    logger.info('A store service is running and will be used to download the needed packages')
    return await connectStoreController(serverJson.connectionOptions) // tslint:disable-line
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return undefined
}
