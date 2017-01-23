import {StrictPnpmOptions, PnpmOptions} from '../types'
import globalBinPath = require('global-bin-path')
import path = require('path')
import semver = require('semver')
import logger from 'pnpm-logger'
import {CACHE_PATH} from './cache'

const DEFAULT_GLOBAL_PATH = path.join(globalBinPath(), 'pnpm-global')

const CAN_PRESERVE_SYMLINKS = semver.satisfies(process.version, '>=6.3.0')

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
  flatTree: false,
  engineStrict: false,
  preserveSymlinks: CAN_PRESERVE_SYMLINKS,
})

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  if (opts && opts.preserveSymlinks && !CAN_PRESERVE_SYMLINKS) {
    logger.warn('The active Node version does not support --preserve-symlinks')
    delete opts.preserveSymlinks
  }
  const extendedOpts = Object.assign({}, defaults(), opts)
  if (extendedOpts.flatTree === true && !extendedOpts.preserveSymlinks) {
    throw new Error('`--preserve-symlinks` and so `--flat-tree` are not supported on your system, make sure you are running on Node ≽ 6.3.0')
  }
  extendedOpts.preserveSymlinks = false // TODO: remove preserve-symlinks related code
  return extendedOpts
}
