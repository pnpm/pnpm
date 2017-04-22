import {StrictPnpmOptions, PnpmOptions} from '../types'
import path = require('path')
import logger from 'pnpm-logger'
import expandTilde from '../fs/expandTilde'
import pnpmPkgJson from '../pnpmPkgJson'

const DEFAULT_LOCAL_REGISTRY = expandTilde('~/.pnpm-registry')

const defaults = (opts: PnpmOptions) => {
  const prefix = process.cwd()
  return <StrictPnpmOptions>{
    fetchRetries: 2,
    fetchRetryFactor: 10,
    fetchRetryMintimeout: 1e4, // 10 seconds
    fetchRetryMaxtimeout: 6e4, // 1 minute
    storePath: '~/.pnpm-store',
    localRegistry: DEFAULT_LOCAL_REGISTRY,
    ignoreScripts: false,
    strictSsl: true,
    tag: 'latest',
    production: process.env.NODE_ENV === 'production',
    bin: path.join(opts.prefix || prefix, 'node_modules', '.bin'),
    prefix,
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
    userAgent: `${pnpmPkgJson.name}/${pnpmPkgJson.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    rawNpmConfig: {},
  }
}

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  if (opts) {
    for (const key in opts) {
      if (opts[key] === undefined) {
        delete opts[key]
      }
    }
  }
  const extendedOpts = Object.assign({}, defaults(opts || {}), opts)
  if (extendedOpts.force) {
    logger.warn('using --force I sure hope you know what you are doing')
  }
  if (extendedOpts.localRegistry !== DEFAULT_LOCAL_REGISTRY) {
    extendedOpts.localRegistry = expandTilde(extendedOpts.localRegistry, extendedOpts.prefix)
  }
  if (extendedOpts.save === false && extendedOpts.saveDev === false && extendedOpts.saveOptional === false) {
    throw new Error('Cannot install with save/saveDev/saveOptional all being equal false')
  }
  extendedOpts.save = extendedOpts.save || !extendedOpts.saveDev && !extendedOpts.saveOptional
  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${pnpmPkgJson.name}/${pnpmPkgJson.version} ${extendedOpts.userAgent}`
  }
  return extendedOpts
}
