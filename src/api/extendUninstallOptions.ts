import path = require('path')
import logger from '@pnpm/logger'
import pnpmPkgJson from '../pnpmPkgJson'
import {LAYOUT_VERSION} from '../fs/modulesController'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import { ReporterFunction } from '../types'

export type UninstallOptions = {
  prefix?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
  storeController: StoreController,
  global?: boolean,
  registry?: string,
  shrinkwrap?: boolean,
  shamefullyFlatten?: boolean,

  reporter?: ReporterFunction,
  lock?: boolean,
  lockStaleDuration?: number,
  locks?: string,
  bin?: string,
  packageManager?: {
    name: string,
    version: string,
  },
}

export type StrictUninstallOptions = UninstallOptions & {
  prefix: string,
  store: string,
  independentLeaves: boolean,
  force: boolean,
  shamefullyFlatten: boolean,
  storeController: StoreController,
  global: boolean,
  registry: string,
  shrinkwrap: boolean,

  lock: boolean,
  lockStaleDuration: number,
  locks: string,
  bin: string,
  packageManager: {
    name: string,
    version: string,
  },
}

const defaults = async (opts: UninstallOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  const prefix = opts.prefix || process.cwd()
  return <StrictUninstallOptions>{
    shamefullyFlatten: false,
    storeController: opts.storeController,
    global: false,
    store: opts.store,
    locks: path.join(opts.store, '_locks'),
    bin: path.join(prefix, 'node_modules', '.bin'),
    prefix,
    force: false,
    lockStaleDuration: 60 * 1000, // 1 minute
    lock: true,
    registry: 'https://registry.npmjs.org/',
    independentLeaves: false,
    packageManager,
    shrinkwrap: true,
  }
}

export default async (
  opts: UninstallOptions,
): Promise<StrictUninstallOptions> => {
  if (opts) {
    for (const key in opts) {
      if (opts[key] === undefined) {
        delete opts[key]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = {...defaultOpts, ...opts, store: defaultOpts.store}
  if (extendedOpts.force) {
    logger.warn('using --force I sure hope you know what you are doing')
  }
  if (extendedOpts.lock === false) {
    logger.warn('using --no-lock I sure hope you know what you are doing')
  }
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  if (extendedOpts.global) {
    const subfolder = LAYOUT_VERSION.toString() + (extendedOpts.independentLeaves ? '_independent_leaves' : '')
    extendedOpts.prefix = path.join(extendedOpts.prefix, subfolder)
  }
  return extendedOpts
}
