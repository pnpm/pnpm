import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import { ReporterFunction } from '../types'

export interface LinkOptions {
  bin?: string,
  force?: boolean,
  forceSharedShrinkwrap?: boolean,
  reporter?: ReporterFunction,
  pinnedVersion?: 'major' | 'minor' | 'patch',
  saveProd?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  shrinkwrap?: boolean,
  storeController: StoreController,
  prefix?: string,
  shamefullyFlatten?: boolean,
  shrinkwrapDirectory?: string,
  independentLeaves?: boolean,
  registries?: Registries,
  store?: string,
}

export type StrictLinkOptions = LinkOptions & {
  bin: string,
  force: boolean,
  forceSharedShrinkwrap: boolean,
  saveDev: boolean,
  saveOptional: boolean,
  saveProd: boolean,
  shrinkwrap: boolean,
  prefix: string,
  shamefullyFlatten: boolean,
  shrinkwrapDirectory: string,
  independentLeaves: boolean,
  registries: Registries,
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
  const extendedOpts = { ...defaultOpts, ...opts, store: defaultOpts.store }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}

async function defaults (opts: LinkOptions) {
  const prefix = opts.prefix || process.cwd()
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    force: false,
    forceSharedShrinkwrap: false,
    independentLeaves: false,
    prefix,
    registries: DEFAULT_REGISTRIES,
    shamefullyFlatten: false,
    shrinkwrap: true,
    shrinkwrapDirectory: opts.shrinkwrapDirectory || prefix,
    store: opts.store,
    storeController: opts.storeController,
  } as StrictLinkOptions
}
