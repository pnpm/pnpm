import {StrictPnpmOptions, PnpmOptions} from '../types'
import globalBinPath = require('global-bin-path')
import path = require('path')
import logger from 'pnpm-logger'
import expandTilde from '../fs/expandTilde'

const DEFAULT_GLOBAL_PATH = path.join(globalBinPath(), 'pnpm-global')
const DEFAULT_LOCAL_REGISTRY = expandTilde('~/.pnpm-registry')

const defaults = () => (<StrictPnpmOptions>{
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4, // 10 seconds
  fetchRetryMaxtimeout: 6e4, // 1 minute
  storePath: '~/.pnpm-store',
  localRegistry: DEFAULT_LOCAL_REGISTRY,
  globalPath: DEFAULT_GLOBAL_PATH,
  ignoreScripts: false,
  strictSsl: true,
  tag: 'latest',
  production: process.env.NODE_ENV === 'production',
  cwd: process.cwd(),
  nodeVersion: process.version,
  force: false,
  depth: -1, // respect everything that is in shrinkwrap.yaml by default
  engineStrict: false,
  metaCache: new Map(),
  networkConcurrency: 16,
  fetchingConcurrency: 16,
  lockStaleDuration: 60 * 1000, // 1 minute
  childConcurrency: 5,
  offline: false,
  registry: 'https://registry.npmjs.org/',
})

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  const extendedOpts = Object.assign({}, defaults(), opts)
  if (extendedOpts.force) {
    logger.warn('using --force I sure hope you know what you are doing')
  }
  if (extendedOpts.localRegistry !== DEFAULT_LOCAL_REGISTRY) {
    extendedOpts.localRegistry = expandTilde(extendedOpts.localRegistry, extendedOpts.cwd)
  }
  if (extendedOpts.save === false && extendedOpts.saveDev === false && extendedOpts.saveOptional === false) {
    throw new Error('Cannot install with save/saveDev/saveOptional all being equal false')
  }
  extendedOpts.save = extendedOpts.save || !extendedOpts.saveDev && !extendedOpts.saveOptional
  return extendedOpts
}
