import {StrictPnpmOptions, PnpmOptions} from '../types'
import {GlobalPath as globalPath, GlobalStorePath} from './constantDefaults'
import {preserveSymlinks} from '../env'
import {LoggerType} from '../logger' // tslint:disable-line

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
  nodeVersion: process.version,
  force: false,
  silent: true,
  depth: 0,
  cacheTTL: 60 * 60 * 24, // 1 day
  flatTree: false,
  engineStrict: false,
})

function getDefaultStorePath () {
  if (preserveSymlinks) return GlobalStorePath
  return 'node_modules/.store'
}

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  opts = opts || {}
  if (opts.flatTree === true && !preserveSymlinks) {
    throw new Error('`--preserve-symlinks` and so `--flat-tree` are not supported on your system, make sure you are running on Node ≽ 6.3.0')
  }
  return Object.assign({}, defaults(), opts)
}
