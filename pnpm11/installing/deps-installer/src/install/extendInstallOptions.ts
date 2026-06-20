import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/config.normalize-registries'
import { parseOverrides, type VersionOverride } from '@pnpm/config.parse-overrides'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { CustomFetcher, CustomResolver, PreResolutionHookContext } from '@pnpm/hooks.types'
import type { ProjectOptions } from '@pnpm/installing.context'
import type { HoistingLimits } from '@pnpm/installing.deps-restorer'
import type { IncludedDependencies } from '@pnpm/installing.modules-yaml'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionPolicyViolation, ResolutionVerifier, WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  AllowedDeprecatedVersions,
  PackageExtension,
  PackageVulnerabilityAudit,
  PeerDependencyRules,
  ReadPackageHook,
  Registries,
  RegistryConfig,
  SupportedArchitectures,
  TrustPolicy,
} from '@pnpm/types'

import { pnpmPkgJson } from '../pnpmPkgJson.js'
import type { ReporterFunction } from '../types.js'

export interface StrictInstallOptions {
  autoConfirmAllPrompts: boolean
  autoInstallPeers: boolean
  autoInstallPeersFromHighestMatch: boolean
  catalogs: Catalogs
  catalogMode: 'strict' | 'prefer' | 'manual'
  cleanupUnusedCatalogs: boolean
  frozenLockfile: boolean
  frozenLockfileIfExists: boolean
  frozenStore: boolean
  enableGlobalVirtualStore: boolean
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
  updateChecksums: boolean
  dedupe: boolean
  ignoreCompatibilityDb: boolean
  ignorePackageManifest: boolean
  /**
   * When true, skip fetching local dependencies (file: protocol pointing to directories).
   * This is used by `pnpm fetch` which only downloads packages from the registry
   * and doesn't need local packages that won't be available (e.g., in Docker builds).
   */
  ignoreLocalPackages: boolean
  preferFrozenLockfile: boolean
  saveWorkspaceProtocol: boolean | 'rolling'
  lockfileCheck?: (prev: LockfileObject, next: LockfileObject) => void
  /**
   * When true, resolve fully but write nothing to disk (no lockfile, no
   * `node_modules`). The before/after wanted lockfiles are returned in the
   * install result's `dryRunResult` so the caller can report what an install
   * would change. Powers `pnpm install --dry-run`.
   */
  dryRun?: boolean
  lockfileIncludeTarballUrl?: boolean
  preferWorkspacePackages: boolean
  preserveWorkspaceProtocol: boolean
  saveCatalogName?: string
  scriptsPrependNodePath: boolean | 'warn-only'
  scriptShell?: string
  shellEmulator: boolean
  storeController: StoreController
  storeDir: string
  reporter: ReporterFunction
  force: boolean
  depth: number
  lockfileDir: string
  modulesDir: string
  configByUri: Record<string, RegistryConfig>
  verifyStoreIntegrity: boolean
  engineStrict: boolean
  allowBuilds?: Record<string, boolean | string>
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  nodeExperimentalPackageMap: boolean
  nodePackageMapType: 'standard' | 'loose'
  nodeVersion?: string
  packageExtensions: Record<string, PackageExtension>
  ignoredOptionalDependencies: string[]
  pnpmfile: string[] | string
  ignorePnpmfile: boolean
  packageManager: {
    name: string
    version: string
  }
  pruneLockfileImporters: boolean
  hooks: {
    readPackage?: ReadPackageHook[]
    preResolution?: Array<(ctx: PreResolutionHookContext) => Promise<void>>
    afterAllResolved?: Array<(lockfile: LockfileObject) => LockfileObject | Promise<LockfileObject>>
    customResolvers?: CustomResolver[]
    customFetchers?: CustomFetcher[]
    calculatePnpmfileChecksum?: () => Promise<string | undefined>
  }
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  strictPeerDependencies: boolean
  include: IncludedDependencies
  includeDirect: IncludedDependencies
  ignoreCurrentSpecifiers: boolean
  ignoreScripts: boolean
  childConcurrency: number
  userAgent: string
  unsafePerm: boolean
  registries: Registries
  namedRegistries?: Record<string, string>
  tag: string
  overrides: Record<string, string>
  ownLifecycleHooksStdio: 'inherit' | 'pipe'
  // We can automatically calculate these
  // unless installation runs on a workspace
  // that doesn't share a lockfile
  workspacePackages?: WorkspacePackages
  pruneStore: boolean
  virtualStoreDir?: string
  globalVirtualStoreDir: string
  dir: string
  symlink: boolean
  enableModulesDir: boolean
  virtualStoreOnly: boolean
  modulesCacheMaxAge: number
  peerDependencyRules: PeerDependencyRules
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowUnusedPatches: boolean
  preferSymlinkedExecutables: boolean
  resolutionMode: 'highest' | 'time-based' | 'lowest-direct'
  resolvePeersFromWorkspaceRoot: boolean
  ignoreWorkspaceCycles: boolean
  disallowWorkspaceCycles: boolean

  publicHoistPattern: string[] | undefined
  hoistPattern: string[] | undefined

  shamefullyHoist: boolean

  global: boolean
  globalBin?: string
  patchedDependencies?: Record<string, string>

  allProjects: ProjectOptions[]
  resolveSymlinksInInjectedDirs: boolean
  dedupeDirectDeps: boolean
  dedupeInjectedDeps: boolean
  dedupePeerDependents: boolean
  dedupePeers: boolean
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

  skipRuntimes: boolean
  supportedArchitectures?: SupportedArchitectures
  hoistWorkspacePackages?: boolean
  virtualStoreDirMaxLength: number
  peersSuffixMaxLength: number
  returnListOfDepsRequiringBuild?: boolean
  injectWorkspacePackages?: boolean
  ci?: boolean
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  /**
   * Resolver-agnostic post-tree gate, invoked between
   * `resolveDependencyTree` and `resolvePeers` inside
   * `resolveDependencies`. Receives the violations the verifier
   * fan-out collected from the freshly-resolved tree. Throwing here
   * unwinds the install before peer-dep resolution runs — nothing on
   * disk has changed, and the (potentially expensive) peer pass is
   * skipped on abort.
   *
   * Intentionally policy-neutral. Each verifier owns its violation
   * codes (`MINIMUM_RELEASE_AGE_VIOLATION`, `TRUST_DOWNGRADE`, …); the
   * install command filters by code to decide what to do. Future
   * resolvers can plug verifiers in without touching this signature.
   */
  handleResolutionPolicyViolations?: (
    violations: readonly ResolutionPolicyViolation[]
  ) => Promise<void>
  /**
   * Resolver-side verifiers that re-check each lockfile-pinned resolution
   * against policies configured upstream (today: at most one,
   * `npm.minimumReleaseAge` in strict mode). Constructed by `createClient`
   * and surfaced via the `createStoreController` return; mutateModules
   * fans out across the list once, right after the lockfile is loaded
   * from disk. Empty when no policy is active.
   */
  resolutionVerifiers: ResolutionVerifier[]
  /**
   * pnpm's on-disk cache directory. When set together with non-empty
   * `resolutionVerifiers`, the lockfile verification result is memoized
   * in `<cacheDir>/lockfile-verified.jsonl` so repeat installs against an
   * unchanged lockfile skip the per-package registry round trip. The
   * record is policy-neutral; each active resolver-side verifier writes
   * its own slot under `verifiers[<key>]`.
   */
  cacheDir?: string
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: string[]
  trustPolicyIgnoreAfter?: number
  /**
   * Skip the lockfile supply-chain verification pass entirely. When
   * true, `verifyLockfileResolutions` is not called even if
   * `resolutionVerifiers` is non-empty — the install trusts the
   * lockfile as-is. Trade-off: a poisoned lockfile (e.g. one a
   * contributor authored under a weaker policy than CI enforces) can
   * slip through. Use only in environments where the lockfile is
   * effectively part of the trusted base — closed-source projects
   * where every commit comes from a trusted author, fully reproducible
   * CI runs against an already-verified lockfile, etc.
   *
   * Added for #11860: on workspaces with thousands of locked entries,
   * the verification pass holds the per-package registry metadata
   * needed for the trust check resident in memory and can OOM CI
   * runners with a 2GB heap cap.
   */
  trustLockfile?: boolean
  packageVulnerabilityAudit?: PackageVulnerabilityAudit
  blockExoticSubdeps?: boolean
  /**
   * Optional alternative install engine. When set, the installer
   * delegates the install to `run` instead of calling `headlessInstall`.
   * The CLI layer constructs it (today: the pacquet binary installed via
   * `configDependencies`, forwarding pnpm's own CLI argv); the installer
   * treats it as an opaque "do the install" hook so it doesn't need to
   * know about pacquet's binary path, CLI surface, or any settings that
   * only pacquet consumes.
   *
   * `supportsResolution` is `true` when the engine can resolve
   * dependencies itself (pacquet >= 0.11.7). When `false` the installer
   * runs its own resolve pass first and the engine only materializes the
   * written lockfile.
   *
   * `run`'s `filterResolvedProgress` tells the helper to drop the
   * engine's own `pnpm:progress status:resolved` events because pnpm
   * already emitted one per package during a preceding lockfileOnly
   * resolve pass. `resolve` tells the engine to do the resolution
   * itself (non-frozen install). The frozen/materialize paths leave
   * both unset.
   */
  runPacquet?: {
    supportsResolution: boolean
    run: (opts?: { filterResolvedProgress?: boolean, resolve?: boolean }) => Promise<void>
  }
  /**
   * If true, `mutateModules` does not emit the per-install `summary` log
   * event. Used by `pnpm add -g` when it runs multiple isolated installs
   * inside a single command and wants to emit a single consolidated
   * summary at the very end instead of one summary per install.
   */
  omitSummaryLog: boolean
  /**
   * URL of a pnpr server that resolves dependencies server-side and serves
   * only the files missing from the client's store.
   */
  pnprServer?: string
}

export type InstallOptions =
  & Partial<StrictInstallOptions>
  & Pick<StrictInstallOptions, 'storeDir' | 'storeController'>

const defaults = (opts: InstallOptions): StrictInstallOptions => {
  const packageManager = opts.packageManager ?? {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  return {
    allowedDeprecatedVersions: {},
    allowUnusedPatches: false,
    autoConfirmAllPrompts: opts.autoConfirmAllPrompts ?? false,
    autoInstallPeers: true,
    autoInstallPeersFromHighestMatch: false,
    catalogs: {},
    childConcurrency: 5,
    confirmModulesPurge: !(opts.autoConfirmAllPrompts || opts.force),
    depth: 0,
    dedupeInjectedDeps: true,
    enableGlobalVirtualStore: false,
    enablePnp: false,
    engineStrict: false,
    force: false,
    forceFullResolution: false,
    frozenLockfile: false,
    frozenStore: false,
    hoistPattern: undefined,
    publicHoistPattern: undefined,
    hooks: {},
    ignoreCurrentSpecifiers: false,
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
    updateChecksums: false,
    nodeVersion: opts.nodeVersion,
    nodeLinker: 'isolated',
    nodeExperimentalPackageMap: false,
    nodePackageMapType: 'standard',
    overrides: {},
    ownLifecycleHooksStdio: 'inherit',
    ignoreCompatibilityDb: false,
    ignorePackageManifest: false,
    ignoreLocalPackages: false,
    packageExtensions: {},
    ignoredOptionalDependencies: [] as string[],
    packageManager,
    preferFrozenLockfile: true,
    preferWorkspacePackages: false,
    preserveWorkspaceProtocol: true,
    pruneLockfileImporters: false,
    pruneStore: false,
    configByUri: {},
    registries: DEFAULT_REGISTRIES,
    resolutionMode: 'highest',
    saveWorkspaceProtocol: 'rolling',
    scriptsPrependNodePath: false,
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
      !process.setgid ||
      process.getuid?.() !== 0,
    catalogMode: 'manual',
    cleanupUnusedCatalogs: false,
    useLockfile: true,
    saveLockfile: true,
    useGitBranchLockfile: false,
    mergeGitBranchLockfiles: false,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    verifyStoreIntegrity: true,
    enableModulesDir: true,
    virtualStoreOnly: false,
    modulesCacheMaxAge: 7 * 24 * 60,
    resolveSymlinksInInjectedDirs: false,
    dedupeDirectDeps: true,
    dedupePeerDependents: true,
    dedupePeers: false,
    resolvePeersFromWorkspaceRoot: true,
    extendNodePath: true,
    ignoreWorkspaceCycles: false,
    disallowWorkspaceCycles: false,
    excludeLinksFromLockfile: false,
    skipRuntimes: false,
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1000,
    blockExoticSubdeps: false,
    omitSummaryLog: false,
    resolutionVerifiers: [] as ResolutionVerifier[],
  } as StrictInstallOptions
}

export interface ProcessedInstallOptions extends StrictInstallOptions {
  readPackageHook?: ReadPackageHook
  parsedOverrides: VersionOverride[]
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

  const defaultOpts = defaults(opts)
  const extendedOpts: ProcessedInstallOptions = {
    ...defaultOpts,
    ...opts,
    storeDir: defaultOpts.storeDir,
    parsedOverrides: parseOverrides(opts.overrides ?? {}, opts.catalogs ?? {}),
  }
  extendedOpts.readPackageHook = createReadPackageHook({
    ignoreCompatibilityDb: extendedOpts.ignoreCompatibilityDb,
    readPackageHook: extendedOpts.hooks?.readPackage,
    overrides: extendedOpts.parsedOverrides,
    lockfileDir: extendedOpts.lockfileDir,
    packageExtensions: extendedOpts.packageExtensions,
    ignoredOptionalDependencies: extendedOpts.ignoredOptionalDependencies,
  })
  if (extendedOpts.virtualStoreOnly && !extendedOpts.enableModulesDir && !extendedOpts.enableGlobalVirtualStore) {
    throw new PnpmError('CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR',
      'Cannot use virtualStoreOnly when enableModulesDir is false (the standard virtual store requires node_modules/.pnpm)')
  }
  if (extendedOpts.virtualStoreOnly) {
    // Ensure .modules.yaml records empty hoist patterns so a subsequent
    // normal install knows hoisting must be redone from scratch.
    extendedOpts.hoistPattern = []
    extendedOpts.publicHoistPattern = []
  }
  if (extendedOpts.lockfileOnly) {
    extendedOpts.ignoreScripts = true
    if (!extendedOpts.useLockfile) {
      throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE',
        `Cannot generate a ${WANTED_LOCKFILE} because lockfile is set to false`)
    }
  }
  if (extendedOpts.frozenStore && extendedOpts.force) {
    throw new PnpmError('CONFIG_CONFLICT_FROZEN_STORE_WITH_FORCE',
      'Cannot use force together with frozenStore: --force re-imports packages into the store, which is opened read-only when frozenStore is enabled')
  }
  if (extendedOpts.frozenStore) {
    // The side-effects cache is written into the store, which frozenStore opens
    // read-only. Caching is an optimization, not a correctness requirement, so
    // force it off rather than failing (the writable seed-build already
    // populated it). Without this, a build under frozenStore (e.g. with the
    // global virtual store disabled) would attempt a store write.
    extendedOpts.sideEffectsCacheWrite = false
  }
  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
  }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  if (extendedOpts.enableGlobalVirtualStore) {
    if (extendedOpts.virtualStoreDir == null) {
      extendedOpts.virtualStoreDir = path.join(extendedOpts.storeDir, 'links')
    }
    extendedOpts.allowBuilds ??= {}
  }
  extendedOpts.globalVirtualStoreDir = extendedOpts.enableGlobalVirtualStore
    ? extendedOpts.virtualStoreDir!
    : path.join(extendedOpts.storeDir, 'links')
  return extendedOpts
}
