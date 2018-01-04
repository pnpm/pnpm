import path = require('path')
import logger from '@pnpm/logger'
import pnpmPkgJson from '../pnpmPkgJson'
import {LAYOUT_VERSION} from '../fs/modulesController'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import { ReporterFunction } from '../types'

export type PruneOptions = {
  prefix?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
  storeController: StoreController,
  global?: boolean,
  registry?: string,

  reporter?: ReporterFunction,
  production?: boolean,
  development?: boolean,
  optional?: boolean,
  bin?: string,
}

export type StrictPruneOptions = PruneOptions & {
  prefix: string,
  store: string,
  independentLeaves: boolean,
  force: boolean,
  storeController: StoreController,
  global: boolean,
  registry: string,
  bin: string,
  production: boolean,
  development: boolean,
  optional: boolean,
}

const defaults = async (opts: PruneOptions) => {
  const prefix = opts.prefix || process.cwd()
  return <StrictPruneOptions>{
    storeController: opts.storeController,
    global: false,
    store: opts.store,
    bin: path.join(prefix, 'node_modules', '.bin'),
    prefix,
    force: false,
    registry: 'https://registry.npmjs.org/',
    independentLeaves: false,
    production: true,
    development: true,
    optional: true,
  }
}

export default async (
  opts: PruneOptions,
): Promise<StrictPruneOptions> => {
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
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  if (extendedOpts.global) {
    const subfolder = LAYOUT_VERSION.toString() + (extendedOpts.independentLeaves ? '_independent_leaves' : '')
    extendedOpts.prefix = path.join(extendedOpts.prefix, subfolder)
  }
  return extendedOpts
}
