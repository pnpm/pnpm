import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { Lockfile } from '@pnpm/lockfile-file'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import { LocalPackages } from '@pnpm/resolver-base'
import { StoreController } from '@pnpm/store-controller-types'
import {
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface StrictInstallOptions {
  forceSharedLockfile: boolean,
  frozenLockfile: boolean,
  extraBinPaths: string[],
  useLockfile: boolean,
  lockfileOnly: boolean,
  preferFrozenLockfile: boolean,
  saveWorkspaceProtocol: boolean,
  storeController: StoreController,
  store: string,
  reporter: ReporterFunction,
  force: boolean,
  update: boolean,
  depth: number,
  resolutionStrategy: 'fast' | 'fewer-dependencies',
  lockfileDirectory: string,
  rawConfig: object,
  verifyStoreIntegrity: boolean,
  engineStrict: boolean,
  nodeVersion: string,
  packageManager: {
    name: string,
    version: string,
  },
  pruneLockfileImporters: boolean,
  hooks: {
    readPackage?: ReadPackageHook,
    afterAllResolved?: (lockfile: Lockfile) => Lockfile,
  },
  sideEffectsCacheRead: boolean,
  sideEffectsCacheWrite: boolean,
  strictPeerDependencies: boolean,
  include: IncludedDependencies,
  ignoreCurrentPrefs: boolean,
  ignoreScripts: boolean,
  childConcurrency: number,
  userAgent: string,
  unsafePerm: boolean,
  registries: Registries,
  lock: boolean,
  lockStaleDuration: number,
  tag: string,
  locks: string,
  ownLifecycleHooksStdio: 'inherit' | 'pipe',
  localPackages: LocalPackages,
  pruneStore: boolean,
  bin: string,
  prefix: string,
  virtualStoreDir?: string,

  hoistPattern: string[] | undefined,
  forceHoistPattern: boolean,

  shamefullyHoist: boolean,
  forceShamefullyHoist: boolean,

  independentLeaves: boolean,
  forceIndependentLeaves: boolean,
}

export type InstallOptions = Partial<StrictInstallOptions> &
  Pick<StrictInstallOptions, 'store' | 'storeController'>

const defaults = async (opts: InstallOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  return {
    childConcurrency: 5,
    depth: 0,
    engineStrict: false,
    force: false,
    forceSharedLockfile: false,
    frozenLockfile: false,
    hoistPattern: undefined,
    hooks: {},
    ignoreCurrentPrefs: false,
    ignoreScripts: false,
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    independentLeaves: false,
    localPackages: {},
    lock: true,
    lockfileDirectory: opts.lockfileDirectory || opts.prefix || process.cwd(),
    lockfileOnly: false,
    locks: path.join(opts.store, '_locks'),
    lockStaleDuration: 5 * 60 * 1000, // 5 minutes
    nodeVersion: process.version,
    ownLifecycleHooksStdio: 'inherit',
    packageManager,
    preferFrozenLockfile: true,
    pruneLockfileImporters: false,
    pruneStore: false,
    rawConfig: {},
    registries: DEFAULT_REGISTRIES,
    resolutionStrategy: 'fast',
    saveWorkspaceProtocol: true,
    shamefullyHoist: false,
    sideEffectsCacheRead: false,
    sideEffectsCacheWrite: false,
    store: opts.store,
    storeController: opts.storeController,
    strictPeerDependencies: false,
    tag: 'latest',
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !(process.getuid && process.setuid &&
        process.getgid && process.setgid) ||
      process.getuid() !== 0,
    update: false,
    useLockfile: true,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    verifyStoreIntegrity: true,
  } as StrictInstallOptions
}

export default async (
  opts: InstallOptions,
): Promise<StrictInstallOptions> => {
  if (opts) {
    for (const key in opts) {
      if (opts[key] === undefined) {
        delete opts[key]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = {
    ...defaultOpts,
    ...opts,
    store: defaultOpts.store,
  }
  if (!extendedOpts.useLockfile && extendedOpts.lockfileOnly) {
    throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE',
      `Cannot generate a ${WANTED_LOCKFILE} because lockfile is set to false`)
  }
  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
  }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  extendedOpts.rawConfig['registry'] = extendedOpts.registries.default // tslint:disable-line:no-string-literal
  return extendedOpts
}
