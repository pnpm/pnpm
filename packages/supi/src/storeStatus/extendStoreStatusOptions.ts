import { Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import { ReporterFunction } from '../types'

export interface StrictStoreStatusOptions {
  lockfileDirectory: string,
  workingDir: string,
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
  bin: string,
}

export type StoreStatusOptions = Partial<StrictStoreStatusOptions> &
  Pick<StrictStoreStatusOptions, 'storeDir'>

const defaults = async (opts: StoreStatusOptions) => {
  const workingDir = opts.workingDir || process.cwd()
  const lockfileDirectory = opts.lockfileDirectory || workingDir
  return {
    bin: path.join(workingDir, 'node_modules', '.bin'),
    force: false,
    forceSharedLockfile: false,
    independentLeaves: false,
    lockfileDirectory,
    registries: DEFAULT_REGISTRIES,
    shamefullyHoist: false,
    storeDir: opts.storeDir,
    useLockfile: true,
    workingDir,
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
