import {StrictPnpmOptions, PnpmOptions} from '../types'
import globalBinPath = require('global-bin-path')
import path = require('path')
import logger from 'pnpm-logger'
import {CACHE_PATH} from './cache'

const DEFAULT_GLOBAL_PATH = path.join(globalBinPath(), 'pnpm-global')

const defaults = () => (<StrictPnpmOptions>{
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4, // 10 seconds
  fetchRetryMaxtimeout: 6e4, // 1 minute
  storePath: '~/.pnpm-store',
  cachePath: CACHE_PATH,
  globalPath: DEFAULT_GLOBAL_PATH,
  ignoreScripts: false,
  linkLocal: false,
  strictSsl: true,
  tag: 'latest',
  production: process.env.NODE_ENV === 'production',
  cwd: process.cwd(),
  nodeVersion: process.version,
  force: false,
  depth: 0,
  cacheTTL: 60 * 60 * 24, // 1 day
  engineStrict: false,
})

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  return Object.assign({}, defaults(), opts)
}
