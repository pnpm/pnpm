import {StrictPnpmOptions, PnpmOptions} from '../types'
import path = require('path')
import logger from 'pnpm-logger'
import expandTilde from '../fs/expandTilde'
import pnpmPkgJson from '../pnpmPkgJson'
import {LAYOUT_VERSION} from '../fs/modulesController'
import normalizeRegistryUrl = require('normalize-registry-url')

const defaults = (opts: PnpmOptions) => {
  const prefix = process.cwd()
  return <StrictPnpmOptions>{
    fetchRetries: 2,
    fetchRetryFactor: 10,
    fetchRetryMintimeout: 1e4, // 10 seconds
    fetchRetryMaxtimeout: 6e4, // 1 minute
    store: '~/.pnpm-store',
    ignoreScripts: false,
    strictSsl: true,
    tag: 'latest',
    production: process.env.NODE_ENV === 'production',
    bin: path.join(opts.prefix || prefix, 'node_modules', '.bin'),
    prefix,
    nodeVersion: process.version,
    force: false,
    depth: 0,
    engineStrict: false,
    metaCache: new Map(),
    networkConcurrency: 16,
    fetchingConcurrency: 16,
    lockStaleDuration: 60 * 1000, // 1 minute
    lock: true,
    childConcurrency: 5,
    offline: false,
    registry: 'https://registry.npmjs.org/',
    userAgent: `${pnpmPkgJson.name}/${pnpmPkgJson.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    rawNpmConfig: {},
    alwaysAuth: false,
    update: false,
    repeatInstallDepth: -1,
    optional: true,
    independentLeaves: false,
  }
}

export default (opts?: PnpmOptions): StrictPnpmOptions => {
  opts = opts || {}
  if (opts) {
    for (const key in opts) {
      if (opts[key] === undefined) {
        delete opts[key]
      }
    }
  }
  if (opts.storePath && !opts.store) {
    logger.warn('the `store-path` config is deprecated. Use `store` instead.')
    opts.store = opts.storePath
  }
  const extendedOpts = Object.assign({}, defaults(opts), opts)
  if (extendedOpts.force) {
    logger.warn('using --force I sure hope you know what you are doing')
  }
  if (extendedOpts.lock === false) {
    logger.warn('using --no-lock I sure hope you know what you are doing')
  }
  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${pnpmPkgJson.name}/${pnpmPkgJson.version} ${extendedOpts.userAgent}`
  }
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  if (extendedOpts.global) {
    const subfolder = LAYOUT_VERSION.toString() + (extendedOpts.independentLeaves ? '_independent_leaves' : '')
    extendedOpts.prefix = path.join(extendedOpts.prefix, subfolder)
  }
  return extendedOpts
}
