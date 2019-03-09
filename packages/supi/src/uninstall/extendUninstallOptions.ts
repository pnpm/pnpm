import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface UninstallOptions {
  lockfileDirectory?: string,
  prefix?: string,
  store: string,
  independentLeaves?: boolean,
  force?: boolean,
  forceSharedLockfile?: boolean,
  useLockfile?: boolean,
  storeController: StoreController,
  registries?: Registries,
  shamefullyFlatten?: boolean,
  sideEffectsCacheRead?: boolean,

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
  lockfileDirectory: string,
  prefix: string,
  store: string,
  independentLeaves: boolean,
  force: boolean,
  forceSharedLockfile: boolean,
  useLockfile: boolean,
  shamefullyFlatten: boolean,
  sideEffectsCacheRead: boolean,
  storeController: StoreController,
  registries: Registries,

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
  const lockfileDirectory = opts.lockfileDirectory || prefix
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    force: false,
    forceSharedLockfile: false,
    independentLeaves: false,
    lock: true,
    lockfileDirectory,
    locks: path.join(opts.store, '_locks'),
    lockStaleDuration: 5 * 60 * 1000, // 5 minutes
    packageManager,
    prefix,
    registries: DEFAULT_REGISTRIES,
    shamefullyFlatten: false,
    sideEffectsCacheRead: false,
    store: opts.store,
    storeController: opts.storeController,
    useLockfile: true,
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
