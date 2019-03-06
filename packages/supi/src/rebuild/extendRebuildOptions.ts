import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface RebuildOptions {
  childConcurrency?: number,
  lockfileDirectory?: string,
  prefix?: string,
  sideEffectsCacheRead?: boolean,
  store: string, // TODO: remove this property
  storeController: StoreController,
  independentLeaves?: boolean,
  force?: boolean,
  forceSharedLockfile?: boolean,
  lockfile?: boolean,
  registries?: Registries,

  reporter?: ReporterFunction,
  production?: boolean,
  development?: boolean,
  optional?: boolean,
  bin?: string,
  rawNpmConfig?: object,
  userAgent?: string,
  packageManager?: {
    name: string,
    version: string,
  },
  unsafePerm?: boolean,
  pending?: boolean,
  shamefullyFlatten?: boolean,
}

export type StrictRebuildOptions = RebuildOptions & {
  childConcurrency?: number,
  lockfileDirectory: string,
  prefix: string,
  store: string,
  sideEffectsCacheRead: boolean,
  independentLeaves: boolean,
  force: boolean,
  forceSharedLockfile: boolean,
  lockfile: boolean,
  registries: Registries,
  bin: string,
  rawNpmConfig: object,
  userAgent: string,
  packageManager: {
    name: string,
    version: string,
  },
  unsafePerm: boolean,
  pending: boolean,
  shamefullyFlatten: boolean,
}

const defaults = async (opts: RebuildOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  const prefix = opts.prefix || process.cwd()
  const lockfileDirectory = opts.lockfileDirectory || prefix
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    childConcurrency: 5,
    development: true,
    force: false,
    forceSharedLockfile: false,
    independentLeaves: false,
    lockfile: true,
    lockfileDirectory,
    optional: true,
    packageManager,
    pending: false,
    prefix,
    production: true,
    rawNpmConfig: {},
    registries: DEFAULT_REGISTRIES,
    shamefullyFlatten: false,
    sideEffectsCacheRead: false,
    store: opts.store,
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !(process.getuid && process.setuid &&
        process.getgid && process.setgid) ||
      process.getuid() !== 0,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
  } as StrictRebuildOptions
}

export default async (
  opts: RebuildOptions,
): Promise<StrictRebuildOptions> => {
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
