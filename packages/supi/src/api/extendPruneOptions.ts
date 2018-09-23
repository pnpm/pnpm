import normalizeRegistryUrl = require('normalize-registry-url')
import { StoreController } from 'package-store'
import path = require('path')
import { ReporterFunction } from '../types'

export interface PruneOptions {
  prefix?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
  shamefullyFlatten?: boolean,
  storeController: StoreController,
  registry?: string,
  shrinkwrap?: boolean,
  shrinkwrapDirectory?: string,

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
  registry: string,
  bin: string,
  production: boolean,
  development: boolean,
  optional: boolean,
  shrinkwrap: boolean,
  shrinkwrapDirectory: string,
}

const defaults = async (opts: PruneOptions) => {
  const prefix = opts.prefix || process.cwd()
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    development: true,
    force: false,
    independentLeaves: false,
    optional: true,
    prefix,
    production: true,
    registry: 'https://registry.npmjs.org/',
    shamefullyFlatten: false,
    shrinkwrap: true,
    shrinkwrapDirectory: prefix,
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
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  return extendedOpts
}
