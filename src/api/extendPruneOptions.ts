import logger from '@pnpm/logger'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import path = require('path')
import {LAYOUT_VERSION} from '../fs/modulesController'
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface PruneOptions {
  prefix?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
  shamefullyFlatten?: boolean,
  storeController: StoreController,
  global?: boolean,
  registry?: string,
  shrinkwrap?: boolean,

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
  shamefullyFlatten: boolean,
  storeController: StoreController,
  global: boolean,
  registry: string,
  bin: string,
  production: boolean,
  development: boolean,
  optional: boolean,
  shrinkwrap: boolean,
}

const defaults = async (opts: PruneOptions) => {
  const prefix = opts.prefix || process.cwd()
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    development: true,
    force: false,
    global: false,
    independentLeaves: false,
    optional: true,
    prefix,
    production: true,
    registry: 'https://registry.npmjs.org/',
    shamefullyFlatten: false,
    shrinkwrap: true,
    store: opts.store,
    storeController: opts.storeController,
  } as StrictPruneOptions
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
