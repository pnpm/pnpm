import path = require('path')
import {StrictPnpmOptions, PnpmOptions} from '../types'
import {GlobalPath as globalPath} from './constantDefaults'
import {preserveSymlinks} from '../env'

const defaults = () => (<StrictPnpmOptions>{
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4, // 10 seconds
  fetchRetryMaxtimeout: 6e4, // 1 minute
  storePath: getDefaultStorePath(),
  globalPath,
  logger: 'pretty',
  ignoreScripts: false,
  linkLocal: false,
  strictSsl: true,
  tag: 'latest',
  production: process.env.NODE_ENV === 'production',
  cwd: process.cwd(),
  force: false,
  silent: true,
  depth: 0,
  cacheTTL: 60 * 60 * 24, // 1 day
})

function getDefaultStorePath () {
  if (preserveSymlinks) return path.join(globalPath, '.store')
  return 'node_modules/.store'
}

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  return Object.assign({}, defaults(), opts)
}
