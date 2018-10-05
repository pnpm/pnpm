import normalizeRegistryUrl = require('normalize-registry-url')
import { StoreController } from 'package-store'
import path = require('path')
import { ReporterFunction } from '../types'

export interface LinkOptions {
  bin?: string,
  force?: boolean,
  reporter?: ReporterFunction,
  saveExact?: boolean,
  savePrefix?: string,
  saveProd?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  shrinkwrap?: boolean,
  storeController: StoreController,
  prefix?: string,
  shamefullyFlatten?: boolean,
  shrinkwrapDirectory?: string,
  independentLeaves?: boolean,
  registry?: string,
  store?: string,
}

export type StrictLinkOptions = LinkOptions & {
  bin: string,
  force: boolean,
  saveExact: boolean,
  saveDev: boolean,
  saveOptional: boolean,
  savePrefix: string,
  saveProd: boolean,
  shrinkwrap: boolean,
  prefix: string,
  shamefullyFlatten: boolean,
  shrinkwrapDirectory: string,
  independentLeaves: boolean,
  registry: string,
  store: string,
}

export async function extendOptions (opts: LinkOptions): Promise<StrictLinkOptions> {
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

async function defaults (opts: LinkOptions) {
  const prefix = opts.prefix || process.cwd()
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    force: false,
    independentLeaves: false,
    prefix,
    registry: 'https://registry.npmjs.org/',
    shamefullyFlatten: false,
    shrinkwrap: true,
    store: opts.store,
    storeController: opts.storeController,
  } as StrictLinkOptions
}
