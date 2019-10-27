import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface StrictRebuildOptions {
  childConcurrency: number,
  extraBinPaths: string[],
  lockfileDir: string,
  sideEffectsCacheRead: boolean,
  storeDir: string, // TODO: remove this property
  storeController: StoreController,
  force: boolean,
  forceSharedLockfile: boolean,
  useLockfile: boolean,
  registries: Registries,
  workingDir: string,

  reporter: ReporterFunction,
  production: boolean,
  development: boolean,
  optional: boolean,
  bin: string,
  rawConfig: object,
  userAgent: string,
  packageManager: {
    name: string,
    version: string,
  },
  unsafePerm: boolean,
  pending: boolean,
  shamefullyHoist: boolean,
}

export type RebuildOptions = Partial<StrictRebuildOptions> &
  Pick<StrictRebuildOptions, 'storeDir' | 'storeController'>

const defaults = async (opts: RebuildOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  const workingDir = opts.workingDir || process.cwd()
  const lockfileDir = opts.lockfileDir || workingDir
  return {
    bin: path.join(workingDir, 'node_modules', '.bin'),
    childConcurrency: 5,
    development: true,
    force: false,
    forceSharedLockfile: false,
    lockfileDir,
    optional: true,
    packageManager,
    pending: false,
    production: true,
    rawConfig: {},
    registries: DEFAULT_REGISTRIES,
    shamefullyHoist: false,
    sideEffectsCacheRead: false,
    storeDir: opts.storeDir,
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !(process.getuid && process.setuid &&
        process.getgid && process.setgid) ||
      process.getuid() !== 0,
    useLockfile: true,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    workingDir,
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
  const extendedOpts = { ...defaultOpts, ...opts, storeDir: defaultOpts.storeDir }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}
