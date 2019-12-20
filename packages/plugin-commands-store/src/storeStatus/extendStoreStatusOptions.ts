import { Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import { ReporterFunction } from '../types'

export interface StrictStoreStatusOptions {
  lockfileDir: string,
  dir: string,
  storeDir: string,
  independentLeaves: boolean,
  force: boolean,
  forceSharedLockfile: boolean,
  useLockfile: boolean,
  registries: Registries,
  shamefullyHoist: boolean,

  reporter: ReporterFunction,
  production: boolean,
  development: boolean,
  optional: boolean,
  binsDir: string,
}

export type StoreStatusOptions = Partial<StrictStoreStatusOptions> &
  Pick<StrictStoreStatusOptions, 'storeDir'>

const defaults = async (opts: StoreStatusOptions) => {
  const dir = opts.dir || process.cwd()
  const lockfileDir = opts.lockfileDir || dir
  return {
    binsDir: path.join(dir, 'node_modules', '.bin'),
    dir,
    force: false,
    forceSharedLockfile: false,
    independentLeaves: false,
    lockfileDir,
    registries: DEFAULT_REGISTRIES,
    shamefullyHoist: false,
    storeDir: opts.storeDir,
    useLockfile: true,
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
  const extendedOpts = { ...defaultOpts, ...opts, storeDir: defaultOpts.storeDir }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}
