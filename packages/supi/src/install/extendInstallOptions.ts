import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { Lockfile } from '@pnpm/lockfile-file'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import normalizeRegistries, { DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import { WorkspacePackages } from '@pnpm/resolver-base'
import { StoreController } from '@pnpm/store-controller-types'
import {
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface StrictInstallOptions {
  forceSharedLockfile: boolean
  frozenLockfile: boolean
  frozenLockfileIfExists: boolean
  enablePnp: boolean
  extraBinPaths: string[]
  useLockfile: boolean
  linkWorkspacePackagesDepth: number
  lockfileOnly: boolean
  preferFrozenLockfile: boolean
  saveWorkspaceProtocol: boolean
  preferWorkspacePackages: boolean
  preserveWorkspaceProtocol: boolean
  scriptShell?: string
  shellEmulator: boolean
  storeController: StoreController
  storeDir: string
  reporter: ReporterFunction
  force: boolean
  update: boolean
  updateMatching?: (pkgName: string) => boolean
  updatePackageManifest?: boolean
  depth: number
  lockfileDir: string
  modulesDir: string
  rawConfig: object
  verifyStoreIntegrity: boolean
  engineStrict: boolean
  nodeVersion: string
  packageManager: {
    name: string
    version: string
  }
  pruneLockfileImporters: boolean
  hooks: {
    readPackage?: ReadPackageHook
    afterAllResolved?: (lockfile: Lockfile) => Lockfile
  }
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  strictPeerDependencies: boolean
  include: IncludedDependencies
  includeDirect: IncludedDependencies
  ignoreCurrentPrefs: boolean
  ignoreScripts: boolean
  childConcurrency: number
  userAgent: string
  unsafePerm: boolean
  registries: Registries
  tag: string
  ownLifecycleHooksStdio: 'inherit' | 'pipe'
  workspacePackages: WorkspacePackages
  pruneStore: boolean
  virtualStoreDir?: string
  dir: string
  symlink: boolean

  hoistPattern: string[] | undefined
  forceHoistPattern: boolean

  shamefullyHoist: boolean
  forceShamefullyHoist: boolean

  global: boolean
  globalBin?: string
}

export type InstallOptions =
  & Partial<StrictInstallOptions>
  & Pick<StrictInstallOptions, 'storeDir' | 'storeController'>

const defaults = async (opts: InstallOptions) => {
  const packageManager = opts.packageManager ?? {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  return {
    childConcurrency: 5,
    depth: 0,
    enablePnp: false,
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
    includeDirect: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: opts.lockfileDir ?? opts.dir ?? process.cwd(),
    lockfileOnly: false,
    nodeVersion: process.version,
    ownLifecycleHooksStdio: 'inherit',
    packageManager,
    preferFrozenLockfile: true,
    preferWorkspacePackages: false,
    preserveWorkspaceProtocol: true,
    pruneLockfileImporters: false,
    pruneStore: false,
    rawConfig: {},
    registries: DEFAULT_REGISTRIES,
    saveWorkspaceProtocol: true,
    shamefullyHoist: false,
    shellEmulator: false,
    sideEffectsCacheRead: false,
    sideEffectsCacheWrite: false,
    symlink: true,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
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
    workspacePackages: {},
  } as StrictInstallOptions
}

export default async (
  opts: InstallOptions
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
    storeDir: defaultOpts.storeDir,
  }
  if (!extendedOpts.useLockfile && extendedOpts.lockfileOnly) {
    throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE',
      `Cannot generate a ${WANTED_LOCKFILE} because lockfile is set to false`)
  }
  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
  }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  extendedOpts.rawConfig['registry'] = extendedOpts.registries.default // eslint-disable-line @typescript-eslint/dot-notation
  return extendedOpts
}
