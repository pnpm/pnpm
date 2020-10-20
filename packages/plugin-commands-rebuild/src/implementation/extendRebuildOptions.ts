import { LogBase } from '@pnpm/logger'
import normalizeRegistries, { DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import path = require('path')
import loadJsonFile = require('load-json-file')

export interface StrictRebuildOptions {
  childConcurrency: number
  extraBinPaths: string[]
  lockfileDir: string
  scriptShell?: string
  sideEffectsCacheRead: boolean
  shellEmulator: boolean
  storeDir: string // TODO: remove this property
  storeController: StoreController
  force: boolean
  forceSharedLockfile: boolean
  useLockfile: boolean
  registries: Registries
  dir: string

  reporter: (logObj: LogBase) => void
  production: boolean
  development: boolean
  optional: boolean
  rawConfig: object
  userAgent: string
  packageManager: {
    name: string
    version: string
  }
  unsafePerm: boolean
  pending: boolean
  shamefullyHoist: boolean
}

export type RebuildOptions = Partial<StrictRebuildOptions> &
Pick<StrictRebuildOptions, 'storeDir' | 'storeController'>

const defaults = async (opts: RebuildOptions) => {
  const packageManager = opts.packageManager ??
    await loadJsonFile<{name: string, version: string}>(path.join(__dirname, '../../package.json'))!
  const dir = opts.dir ?? process.cwd()
  const lockfileDir = opts.lockfileDir ?? dir
  return {
    childConcurrency: 5,
    development: true,
    dir,
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
    shellEmulator: false,
    sideEffectsCacheRead: false,
    storeDir: opts.storeDir,
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !(process.getuid && process.setuid &&
        process.getgid && process.setgid) ||
      process.getuid() !== 0,
    useLockfile: true,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
  } as StrictRebuildOptions
}

export default async (
  opts: RebuildOptions
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
