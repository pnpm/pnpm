import logger from '@pnpm/logger'
import { ReadPackageHook } from '@pnpm/types'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import path = require('path')
import {LAYOUT_VERSION} from '../fs/modulesController'
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface StoreStatusOptions {
  prefix?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
  global?: boolean,
  registry?: string,
  shrinkwrap?: boolean,

  reporter?: ReporterFunction,
  production?: boolean,
  development?: boolean,
  optional?: boolean,
  bin?: string,
  shamefullyFlatten?: boolean,
}

export type StrictStoreStatusOptions = StoreStatusOptions & {
  prefix: string,
  store: string,
  independentLeaves: boolean,
  force: boolean,
  global: boolean,
  registry: string,
  bin: string,
  shrinkwrap: boolean,
  shamefullyFlatten: boolean,
}

const defaults = async (opts: StoreStatusOptions) => {
  const prefix = opts.prefix || process.cwd()
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    force: false,
    global: false,
    independentLeaves: false,
    prefix,
    registry: 'https://registry.npmjs.org/',
    shamefullyFlatten: false,
    shrinkwrap: true,
    store: opts.store,
  } as StrictStoreStatusOptions
}

export default async (
  opts: StoreStatusOptions,
): Promise<StrictStoreStatusOptions> => {
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
