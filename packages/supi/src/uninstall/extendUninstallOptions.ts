import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface UninstallOptions {
  prefix?: string,
  shrinkwrapDirectory?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
  forceSharedShrinkwrap?: boolean,
  storeController: StoreController,
  registries?: Registries,
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
  forceSharedShrinkwrap: boolean,
  shamefullyFlatten: boolean,
  shrinkwrapDirectory: string,
  storeController: StoreController,
  registries: Registries,
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
  const shrinkwrapDirectory = opts.shrinkwrapDirectory || prefix
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    force: false,
    forceSharedShrinkwrap: false,
    independentLeaves: false,
    lock: true,
    lockStaleDuration: 5 * 60 * 1000, // 5 minutes
    locks: path.join(opts.store, '_locks'),
    packageManager,
    prefix,
    registries: DEFAULT_REGISTRIES,
    shamefullyFlatten: false,
    shrinkwrap: true,
    shrinkwrapDirectory,
    store: opts.store,
    storeController: opts.storeController,
  } as StrictUninstallOptions
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
  const extendedOpts = { ...defaultOpts, ...opts, store: defaultOpts.store }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}
