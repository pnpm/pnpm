import normalizeRegistryUrl = require('normalize-registry-url')
import path = require('path')
import { ReporterFunction } from '../types'

export interface StoreStatusOptions {
  prefix?: string,
  shrinkwrapDirectory?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
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
  shrinkwrapDirectory: string,
  independentLeaves: boolean,
  force: boolean,
  registry: string,
  bin: string,
  shrinkwrap: boolean,
  shamefullyFlatten: boolean,
}

const defaults = async (opts: StoreStatusOptions) => {
  const prefix = opts.prefix || process.cwd()
  const shrinkwrapDirectory = opts.shrinkwrapDirectory || prefix
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    force: false,
    independentLeaves: false,
    prefix,
    registry: 'https://registry.npmjs.org/',
    shamefullyFlatten: false,
    shrinkwrap: true,
    shrinkwrapDirectory,
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
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  return extendedOpts
}
