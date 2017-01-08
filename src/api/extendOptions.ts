import {StrictPnpmOptions, PnpmOptions} from '../types'
import {DEFAULT_GLOBAL_PATH, DEFAULT_GLOBAL_STORE_PATH} from './constantDefaults'
import {LoggerType} from '../logger' // tslint:disable-line
import semver = require('semver')

const CAN_PRESERVE_SYMLINKS = semver.satisfies(process.version, '>=6.3.0')

const defaults = () => (<StrictPnpmOptions>{
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4, // 10 seconds
  fetchRetryMaxtimeout: 6e4, // 1 minute
  storePath: DEFAULT_GLOBAL_STORE_PATH,
  globalPath: DEFAULT_GLOBAL_PATH,
  logger: 'pretty',
  ignoreScripts: false,
  linkLocal: false,
  strictSsl: true,
  tag: 'latest',
  production: process.env.NODE_ENV === 'production',
  cwd: process.cwd(),
  nodeVersion: process.version,
  force: false,
  silent: true,
  depth: 0,
  cacheTTL: 60 * 60 * 24, // 1 day
  flatTree: false,
  engineStrict: false,
  preserveSymlinks: CAN_PRESERVE_SYMLINKS,
})

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  if (opts && opts.preserveSymlinks && !CAN_PRESERVE_SYMLINKS) {
    console.warn('The active Node version does not support --preserve-symlinks')
    delete opts.preserveSymlinks
  }
  const extendedOpts = Object.assign({}, defaults(), opts)
  if (extendedOpts.flatTree === true && !extendedOpts.preserveSymlinks) {
    throw new Error('`--preserve-symlinks` and so `--flat-tree` are not supported on your system, make sure you are running on Node ≽ 6.3.0')
  }
  return extendedOpts
}
