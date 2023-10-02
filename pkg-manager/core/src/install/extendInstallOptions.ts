import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { type ProjectOptions } from '@pnpm/get-context'
import { type HoistingLimits } from '@pnpm/headless'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import { type Lockfile } from '@pnpm/lockfile-file'
import { type IncludedDependencies } from '@pnpm/modules-yaml'
import { normalizeRegistries, DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import { type WorkspacePackages } from '@pnpm/resolver-base'
import { type StoreController } from '@pnpm/store-controller-types'
import {
  type AllowedDeprecatedVersions,
  type PackageExtension,
  type PeerDependencyRules,
  type ReadPackageHook,
  type Registries,
} from '@pnpm/types'
import { pnpmPkgJson } from '../pnpmPkgJson'
import { type ReporterFunction } from '../types'
import { type PreResolutionHookContext } from '@pnpm/hooks.types'

export interface StrictInstallOptions {
  autoInstallPeers: boolean
  forceSharedLockfile: boolean
  frozenLockfile: boolean
  frozenLockfileIfExists: boolean
  enablePnp: boolean
  extraBinPaths: string[]
  extraEnv: Record<string, string>
  hoistingLimits?: HoistingLimits
  externalDependencies?: Set<string>
  useLockfile: boolean
  saveLockfile: boolean
  useGitBranchLockfile: boolean
  mergeGitBranchLockfiles: boolean
  linkWorkspacePackagesDepth: number
  lockfileOnly: boolean
  forceFullResolution: boolean
  fixLockfile: boolean
  dedupe: boolean
  ignoreCompatibilityDb: boolean
  ignoreDepScripts: boolean
  ignorePackageManifest: boolean
  preferFrozenLockfile: boolean
  saveWorkspaceProtocol: boolean | 'rolling'
  lockfileCheck?: (prev: Lockfile, next: Lockfile) => void
  lockfileIncludeTarballUrl: boolean
  preferWorkspacePackages: boolean
  preserveWorkspaceProtocol: boolean
  scriptsPrependNodePath: boolean | 'warn-only'
  scriptShell?: string
  shellEmulator: boolean
  storeController: StoreController
  storeDir: string
  reporter: ReporterFunction
  force: boolean
  forcePublicHoistPattern: boolean
  depth: number
  lockfileDir: string
  modulesDir: string
  rawConfig: Record<string, any> // eslint-disable-line @typescript-eslint/no-explicit-any
  verifyStoreIntegrity: boolean
  engineStrict: boolean
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  nodeExecPath?: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  nodeVersion: string
  packageExtensions: Record<string, PackageExtension>
  packageManager: {
    name: string
    version: string
  }
  pruneLockfileImporters: boolean
  hooks: {
    readPackage?: ReadPackageHook[]
    preResolution?: (ctx: PreResolutionHookContext) => Promise<void>
    afterAllResolved?: Array<(lockfile: Lockfile) => Lockfile | Promise<Lockfile>>
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
  overrides: Record<string, string>
  ownLifecycleHooksStdio: 'inherit' | 'pipe'
  workspacePackages: WorkspacePackages
  pruneStore: boolean
  virtualStoreDir?: string
  dir: string
  symlink: boolean
  enableModulesDir: boolean
  modulesCacheMaxAge: number
  peerDependencyRules: PeerDependencyRules
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowNonAppliedPatches: boolean
  preferSymlinkedExecutables: boolean
  resolutionMode: 'highest' | 'time-based' | 'lowest-direct'
  resolvePeersFromWorkspaceRoot: boolean
  ignoreWorkspaceCycles: boolean

  publicHoistPattern: string[] | undefined
  hoistPattern: string[] | undefined
  forceHoistPattern: boolean

  shamefullyHoist: boolean
  forceShamefullyHoist: boolean

  global: boolean
  globalBin?: string
  patchedDependencies?: Record<string, string>

  allProjects: ProjectOptions[]
  resolveSymlinksInInjectedDirs: boolean
  dedupeDirectDeps: boolean
  dedupePeerDependents: boolean
  extendNodePath: boolean
  excludeLinksFromLockfile: boolean
  confirmModulesPurge: boolean
  /**
   * Don't relink local directory dependencies if they are not hard linked from the local directory.
   *
   * This option was added to fix an issue with Bit CLI.
   * Bit compile adds dist directories to the injected dependencies, so if pnpm were to relink them,
   * the dist directories would be deleted.
   *
   * The option might be used in the future to improve performance.
   */
  disableRelinkLocalDirDeps: boolean
}

export type InstallOptions =
  & Partial<StrictInstallOptions>
  & Pick<StrictInstallOptions, 'storeDir' | 'storeController'>

const defaults = (opts: InstallOptions) => {
  const packageManager = opts.packageManager ?? {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  return {
    allowedDeprecatedVersions: {},
    allowNonAppliedPatches: false,
    autoInstallPeers: true,
    childConcurrency: 5,
    confirmModulesPurge: !opts.force,
    depth: 0,
    enablePnp: false,
    engineStrict: false,
    force: false,
    forceFullResolution: false,
    forceSharedLockfile: false,
    frozenLockfile: false,
    hoistPattern: undefined,
    publicHoistPattern: undefined,
    hooks: {},
    ignoreCurrentPrefs: false,
    ignoreDepScripts: false,
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
    nodeLinker: 'isolated',
    overrides: {},
    ownLifecycleHooksStdio: 'inherit',
    ignoreCompatibilityDb: false,
    ignorePackageManifest: false,
    packageExtensions: {},
    packageManager,
    preferFrozenLockfile: true,
    preferWorkspacePackages: false,
    preserveWorkspaceProtocol: true,
    pruneLockfileImporters: false,
    pruneStore: false,
    rawConfig: {},
    registries: DEFAULT_REGISTRIES,
    resolutionMode: 'lowest-direct',
    saveWorkspaceProtocol: 'rolling',
    lockfileIncludeTarballUrl: false,
    scriptsPrependNodePath: false,
    shamefullyHoist: false,
    shellEmulator: false,
    sideEffectsCacheRead: false,
    sideEffectsCacheWrite: false,
    symlink: true,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    strictPeerDependencies: true,
    tag: 'latest',
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !process.setgid ||
      process.getuid() !== 0,
    useLockfile: true,
    saveLockfile: true,
    useGitBranchLockfile: false,
    mergeGitBranchLockfiles: false,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    verifyStoreIntegrity: true,
    workspacePackages: {},
    enableModulesDir: true,
    modulesCacheMaxAge: 7 * 24 * 60,
    resolveSymlinksInInjectedDirs: false,
    dedupeDirectDeps: true,
    dedupePeerDependents: true,
    resolvePeersFromWorkspaceRoot: true,
    extendNodePath: true,
    ignoreWorkspaceCycles: false,
    excludeLinksFromLockfile: false,
  } as StrictInstallOptions
}

export type ProcessedInstallOptions = StrictInstallOptions & {
  readPackageHook?: ReadPackageHook
}

export function extendOptions (
  opts: InstallOptions
): ProcessedInstallOptions {
  if (opts) {
    for (const key in opts) {
      if (opts[key as keyof InstallOptions] === undefined) {
        delete opts[key as keyof InstallOptions]
      }
    }
  }
  if (opts.onlyBuiltDependencies && opts.neverBuiltDependencies) {
    throw new PnpmError('CONFIG_CONFLICT_BUILT_DEPENDENCIES', 'Cannot have both neverBuiltDependencies and onlyBuiltDependencies')
  }
  const defaultOpts = defaults(opts)
  const extendedOpts: ProcessedInstallOptions = {
    ...defaultOpts,
    ...opts,
    storeDir: defaultOpts.storeDir,
  }
  extendedOpts.readPackageHook = createReadPackageHook({
    ignoreCompatibilityDb: extendedOpts.ignoreCompatibilityDb,
    readPackageHook: extendedOpts.hooks?.readPackage,
    overrides: extendedOpts.overrides,
    lockfileDir: extendedOpts.lockfileDir,
    packageExtensions: extendedOpts.packageExtensions,
    peerDependencyRules: extendedOpts.peerDependencyRules,
  })
  if (extendedOpts.lockfileOnly) {
    extendedOpts.ignoreScripts = true
    if (!extendedOpts.useLockfile) {
      throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE',
        `Cannot generate a ${WANTED_LOCKFILE} because lockfile is set to false`)
    }
  }
  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
  }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  extendedOpts.rawConfig['registry'] = extendedOpts.registries.default
  return extendedOpts
}
