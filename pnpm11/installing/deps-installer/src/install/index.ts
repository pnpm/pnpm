import path from 'node:path'

import { linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { buildSelectedPkgs } from '@pnpm/building.after-install'
import { buildModules, type DepsStateCache, linkBinsOfDependencies } from '@pnpm/building.during-install'
import { createAllowBuildFunction, isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import { mergeCatalogs } from '@pnpm/catalogs.config'
import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { type CatalogResultMatcher, matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import {
  LAYOUT_VERSION,
  LOCKFILE_MAJOR_VERSION,
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  ignoredScriptsLogger,
  stageLogger,
  summaryLogger,
  unusedOverrideLogger,
} from '@pnpm/core-loggers'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import {
  makeNodePackageMapOption,
  makeNodeRequireOption,
  runLifecycleHook,
  runLifecycleHooksConcurrently,
  type RunLifecycleHooksConcurrentlyOptions,
} from '@pnpm/exec.lifecycle'
import { getContext, type PnpmContext } from '@pnpm/installing.context'
import {
  type DependenciesGraph,
  type DependenciesGraphNode,
  getWantedDependencies,
  type PinnedVersion,
  resolveDependencies,
  type UpdateMatchingFunction,
  type WantedDependency,
} from '@pnpm/installing.deps-resolver'
import { extendProjectsWithTargetDirs, headlessInstall, type InstallationResultStats } from '@pnpm/installing.deps-restorer'
import { readModulesManifest, type StrictModules, writeModulesManifest } from '@pnpm/installing.modules-yaml'
import {
  type CatalogSnapshots,
  cleanGitBranchLockfiles,
  getWantedLockfileName,
  isEmptyLockfile,
  type LockfileObject,
  type PackageSnapshot,
  type ProjectSnapshot,
  readEnvLockfile,
  readWantedLockfile,
  readWantedLockfileFile,
  writeCurrentLockfile,
  writeEnvLockfile,
  writeLockfiles,
  writeWantedLockfile,
} from '@pnpm/lockfile.fs'
import { getPreferredVersionsFromLockfileAndManifests } from '@pnpm/lockfile.preferred-versions'
import {
  calcPatchHashes,
  createOverridesMapFromParsed,
  getOutdatedLockfileSetting,
} from '@pnpm/lockfile.settings-checker'
import { PACKAGE_MAP_FILENAME, writePackageMap, writePnpFile } from '@pnpm/lockfile.to-pnp'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { allProjectsAreUpToDate, satisfiesPackageManifest } from '@pnpm/lockfile.verification'
import { globalInfo, logger, streamParser } from '@pnpm/logger'
import { groupPatchedDependencies, type PatchGroupRecord } from '@pnpm/patching.config'
import { createVersionSpecFromResolvedVersion, getAllDependenciesFromManifest, getAllUniqueSpecs } from '@pnpm/pkg-manifest.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type {
  PreferredVersions,
  ResolutionPolicyViolation,
} from '@pnpm/resolving.resolver-base'
import type {
  AllowBuild,
  DependenciesField,
  DependencyManifest,
  DepPath,
  IgnoredBuilds,
  PeerDependencyIssues,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
  ReadPackageHook,
} from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { isSubdir } from 'is-subdir'
import pLimit from 'p-limit'
import { clone, isEmpty, map as mapValues, pipeWith, props } from 'ramda'
import semver from 'semver'

import { parseWantedDependencies } from '../parseWantedDependencies.js'
import { removeDeps } from '../uninstall/removeDeps.js'
import { CatalogVersionMismatchError } from './checkCompatibility/CatalogVersionMismatchError.js'
import { checkCustomResolverForceResolve } from './checkCustomResolverForceResolve.js'
import {
  extendOptions,
  type InstallOptions,
  type ProcessedInstallOptions as StrictInstallOptions,
} from './extendInstallOptions.js'
import { linkPackages } from './link.js'
import { reportPeerDependencyIssues } from './reportPeerDependencyIssues.js'
import { validateModules } from './validateModules.js'
import { verifyLockfileResolutions } from './verifyLockfileResolutions.js'
import { writeLockfilesAndRecordVerified } from './writeLockfilesAndRecordVerified.js'
import { writeWantedLockfileAndRecordVerified } from './writeWantedLockfileAndRecordVerified.js'

class LockfileConfigMismatchError extends PnpmError {
  constructor (outdatedLockfileSettingName: string) {
    super('LOCKFILE_CONFIG_MISMATCH',
      `Cannot proceed with the frozen installation. The current "${outdatedLockfileSettingName!}" configuration doesn't match the value found in the lockfile`, {
        hint: 'Update your lockfile using "pnpm install --no-frozen-lockfile"',
      })
  }
}

const BROKEN_LOCKFILE_INTEGRITY_ERRORS = new Set([
  'ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE',
  'ERR_PNPM_TARBALL_INTEGRITY',
])

const DEV_PREINSTALL = 'pnpm:devPreinstall'

interface InstallMutationOptions {
  update?: boolean
  updateToLatest?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest?: boolean
}

export interface InstallDepsMutation extends InstallMutationOptions {
  mutation: 'install'
  pruneDirectDependencies?: boolean
}

export interface InstallSomeDepsMutation extends InstallMutationOptions {
  allowNew?: boolean
  dependencySelectors: string[]
  mutation: 'installSome'
  peer?: boolean
  pruneDirectDependencies?: boolean
  pinnedVersion?: PinnedVersion
  targetDependenciesField?: DependenciesField
}

export interface UninstallSomeDepsMutation {
  mutation: 'uninstallSome'
  dependencyNames: string[]
  targetDependenciesField?: DependenciesField
}

export type DependenciesMutation = InstallDepsMutation | InstallSomeDepsMutation | UninstallSomeDepsMutation

type Opts = Omit<InstallOptions, 'allProjects'> & {
  preferredVersions?: PreferredVersions
  pruneDirectDependencies?: boolean
  binsDir?: string
} & InstallMutationOptions

export interface InstallResult {
  /**
   * A partial of new or updated catalog config entries. A change will be
   * produced if a dependency using the catalog protocol was newly added or
   * updated during this install. To obtain the full catalog, callers should
   * merge this object with the current catalog configs in pnpm-workspace.yaml.
   */
  updatedCatalogs: Catalogs | undefined
  updatedManifest: ProjectManifest
  ignoredBuilds: IgnoredBuilds | undefined
  /** Forwarded from {@link MutateModulesResult.resolutionPolicyViolations}. */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  /** Forwarded from {@link MutateModulesResult.dryRunResult}. */
  dryRunResult?: DryRunInstallResult
}

export async function install (
  manifest: ProjectManifest,
  opts: Opts
): Promise<InstallResult> {
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir

  // When a pnpr server is configured, use server-side resolution
  // instead of the normal resolution flow. The pnpr protocol resolves
  // with overrides but does not report which selectors matched; the
  // unused-override warning is computed by scanning the resolved
  // lockfile inside installViaPnprServer.
  if (opts.pnprServer) {
    return installViaPnprServer(manifest, rootDir, opts)
  }

  const { updatedCatalogs, updatedProjects: projects, ignoredBuilds, resolutionPolicyViolations, dryRunResult } = await mutateModules(
    [
      {
        mutation: 'install',
        pruneDirectDependencies: opts.pruneDirectDependencies,
        rootDir,
        update: opts.update,
        updateMatching: opts.updateMatching,
        updateToLatest: opts.updateToLatest,
        updatePackageManifest: opts.updatePackageManifest,
      },
    ],
    {
      ...opts,
      allProjects: [{
        buildIndex: 0,
        manifest,
        rootDir,
        binsDir: opts.binsDir,
      }],
    }
  )
  return { updatedCatalogs, updatedManifest: projects[0].manifest, ignoredBuilds, resolutionPolicyViolations, dryRunResult }
}

interface ProjectToBeInstalled {
  id: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir: string
  rootDir: ProjectRootDir
}

export type MutatedProject = DependenciesMutation & { rootDir: ProjectRootDir }

export type MutateModulesOptions = InstallOptions & {
  preferredVersions?: PreferredVersions
  hooks?: {
    readPackage?: ReadPackageHook[] | ReadPackageHook
  } | InstallOptions['hooks']
}

export interface MutateModulesInSingleProjectResult {
  updatedCatalogs: Catalogs | undefined
  updatedProject: UpdatedProject
  ignoredBuilds: IgnoredBuilds | undefined
  /** Forwarded from {@link MutateModulesResult.resolutionPolicyViolations}. */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  /** Forwarded from {@link MutateModulesResult.dryRunResult}. */
  dryRunResult?: DryRunInstallResult
}

export async function mutateModulesInSingleProject (
  project: MutatedProject & {
    binsDir?: string
    manifest: ProjectManifest
    rootDir: ProjectRootDir
    modulesDir?: string
  },
  maybeOpts: Omit<MutateModulesOptions, 'allProjects'> & InstallMutationOptions
): Promise<MutateModulesInSingleProjectResult> {
  const result = await mutateModules(
    [
      {
        ...project,
        update: maybeOpts.update,
        updateToLatest: maybeOpts.updateToLatest,
        updateMatching: maybeOpts.updateMatching,
        updatePackageManifest: maybeOpts.updatePackageManifest,
      } as MutatedProject,
    ],
    {
      ...maybeOpts,
      allProjects: [{
        buildIndex: 0,
        ...project,
      }],
    }
  )
  return {
    updatedCatalogs: result.updatedCatalogs,
    updatedProject: result.updatedProjects[0],
    ignoredBuilds: result.ignoredBuilds,
    resolutionPolicyViolations: result.resolutionPolicyViolations,
    dryRunResult: result.dryRunResult,
  }
}

export interface MutateModulesResult {
  updatedCatalogs?: Catalogs
  updatedProjects: UpdatedProject[]
  stats: InstallationResultStats
  depsRequiringBuild?: DepPath[]
  ignoredBuilds: IgnoredBuilds | undefined
  /**
   * Resolver-policy violations the post-resolution scan found in the
   * freshly-resolved lockfile. Each violation carries a verifier code
   * (e.g. `MINIMUM_RELEASE_AGE_VIOLATION`, `TRUST_DOWNGRADE`); the
   * install command filters by code to decide what to do (persist to
   * `minimumReleaseAgeExclude`, log, etc.). Empty array when no
   * verifier reported a violation or no policy was active.
   */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  /**
   * Present only for a `dryRun` install: the before/after wanted lockfiles
   * the resolve produced without writing, for the caller to diff.
   */
  dryRunResult?: DryRunInstallResult
}

const pickCatalogSpecifier: CatalogResultMatcher<string | undefined> = {
  found: (found) =>
    found.resolution.specifier,
  misconfiguration: () => undefined,
  unused: () => undefined,
}

export async function mutateModules (
  projects: MutatedProject[],
  maybeOpts: MutateModulesOptions
): Promise<MutateModulesResult> {
  const reporter = maybeOpts?.reporter
  const detachReporter = (reporter != null) && typeof reporter === 'function'
    ? () => {
      streamParser.removeListener('data', reporter)
    }
    : () => {}
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  const opts = extendOptions(maybeOpts)

  // When a pnpr server is configured, use server-side resolution. The pnpr server
  // path supports `install`, `installSome` (pnpm add), and `uninstallSome`
  // (pnpm remove). Mutations that need full client-side resolution (update
  // flags) still fall through to the normal flow. The unused-override
  // warning is computed by scanning the resolved lockfile — see the
  // comment in `installViaPnprServer`.
  if (opts.pnprServer && canUsePnprForMutations(projects)) {
    const pnprResult = await mutateModulesViaPnpr(projects, opts)
    if (pnprResult) return pnprResult
  }

  const allowBuild = createAllowBuildFunction(opts)

  if (!opts.include.dependencies && opts.include.optionalDependencies) {
    throw new PnpmError('OPTIONAL_DEPS_REQUIRE_PROD_DEPS', 'Optional dependencies cannot be installed without production dependencies')
  }

  const installsOnly = allMutationsAreInstalls(projects)
  if (!installsOnly) opts.strictPeerDependencies = false
  const rootProjectManifest = opts.allProjects.find(({ rootDir }) => rootDir === opts.lockfileDir)?.manifest ??
    // When running install/update on a subset of projects, the root project might not be included,
    // so reading its manifest explicitly here.
    await safeReadProjectManifestOnly(opts.lockfileDir)

  let ctx = await getContext(opts)

  if (!opts.lockfileOnly && ctx.modulesFile != null) {
    const { purged } = await validateModules(ctx.modulesFile, Object.values(ctx.projects), {
      forceNewModules: installsOnly,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
      modulesDir: opts.modulesDir ?? 'node_modules',
      registries: opts.registries,
      storeDir: opts.storeDir,
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      confirmModulesPurge: opts.confirmModulesPurge && !opts.ci,

      hoistPattern: opts.hoistPattern,
      currentHoistPattern: ctx.currentHoistPattern,

      publicHoistPattern: opts.publicHoistPattern,
      currentPublicHoistPattern: ctx.currentPublicHoistPattern,
      global: opts.global,
    })
    if (purged) {
      ctx = await getContext(opts)
    }
  }

  // Re-validate every entry in the lockfile against the policies the
  // resolver chain was built with (today: minimumReleaseAge in strict mode
  // via the npm verifier; the abstraction supports other resolvers
  // attaching their own verifiers). The threat model is a lockfile that
  // someone else resolved — committed to the repo, restored from a CI
  // cache, etc. — bypassing the local resolver's policy filters; the local
  // resolver's own filters already cover fresh resolution.
  //
  // The verification is kicked off here, right after the lockfile is loaded,
  // but not awaited inline — it would otherwise block every later install
  // stage on per-entry registry round trips. Its synchronous prologue (cache
  // lookup, lockfile hashing, candidate collection) runs now against the
  // pristine lockfile, so the async fan-out reads a stable snapshot even
  // while the install mutates `ctx.wantedLockfile` concurrently. The verdict
  // is reconciled with the install in `settleInstall`: a failure aborts the
  // install even mid-flight, and an install that finishes first is held back
  // until the verdict arrives.
  //
  // Skipped when we already know pacquet will run the install: pacquet
  // applies the same resolver-policy gate (port of this function) whether
  // it materializes a frozen lockfile or re-resolves from the manifests,
  // so re-running here would duplicate the work — and for
  // `minimumReleaseAge` in strict mode each lockfile entry is an HTTP
  // probe.
  //
  // The predicate mirrors every short-circuit `tryFrozenInstall` checks
  // before reaching the pacquet branch: anything that would make it
  // return null, throw, or fall through to the JS path must keep
  // verification on. The optimistic `preferFrozenLockfile` path decides
  // whether to delegate later (based on `allProjectsAreUpToDate`), which
  // isn't known here — so verification still runs in that window, the
  // duplicate is bounded to it.
  const willDelegateToPacquet = opts.runPacquet != null &&
    opts.useLockfile &&
    !opts.useGitBranchLockfile &&
    !opts.mergeGitBranchLockfiles &&
    !isCheckOnlyInstall(opts) &&
    opts.enableModulesDir &&
    installsOnly &&
    !opts.lockfileOnly &&
    !opts.fixLockfile &&
    !opts.dedupe &&
    !ctx.lockfileHadConflicts &&
    (
      // Frozen materialization: pacquet reads the existing lockfile and
      // re-applies the resolver-policy gate as it walks it.
      (ctx.existsNonEmptyWantedLockfile &&
        (opts.frozenLockfile === true || opts.frozenLockfileIfExists === true)) ||
      // Resolving install: pacquet (>= 0.11.7) re-resolves from the
      // manifests itself — applying the policy during fresh resolution —
      // so the existing lockfile entries verified here would just be
      // discarded. If a policy handler is active, keep resolution in pnpm
      // so violations can be returned to the command layer.
      (opts.saveLockfile && opts.runPacquet.supportsResolution && opts.frozenLockfile !== true && opts.nodeLinker !== 'hoisted' && opts.handleResolutionPolicyViolations == null)
    )
  let verifyLockfilePromise: Promise<void> | undefined
  if (!willDelegateToPacquet && !opts.trustLockfile) {
    const cacheActive = opts.cacheDir != null && opts.resolutionVerifiers.length > 0
    const wantedLockfilePath = cacheActive
      ? path.resolve(ctx.lockfileDir, await getWantedLockfileName({
        useGitBranchLockfile: opts.useGitBranchLockfile,
        mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
      }))
      : undefined
    verifyLockfilePromise = verifyLockfileResolutions(ctx.wantedLockfile, opts.resolutionVerifiers, {
      cacheDir: opts.cacheDir,
      lockfilePath: wantedLockfilePath,
    })
    // Keep the rejection from going unhandled in the window before
    // `settleInstall` awaits the verdict — a preResolution hook or the
    // install kickoff below could throw and bail out before we get there.
    verifyLockfilePromise.catch(() => {})
  }

  // Gate passed down to the build phase: fetching and linking overlap with
  // verification, but no dependency lifecycle script may run until the verdict
  // is in. Awaiting the promise here throws if verification failed, aborting
  // before any script executes. `settleInstall` is the catch-all that still
  // reconciles the verdict on paths that never reach the build phase.
  const verifyLockfile = verifyLockfilePromise && (() => verifyLockfilePromise)

  if (opts.hooks.preResolution) {
    for (const preResolution of opts.hooks.preResolution) {
      // eslint-disable-next-line no-await-in-loop
      await preResolution({
        currentLockfile: ctx.currentLockfile,
        wantedLockfile: ctx.wantedLockfile,
        existsCurrentLockfile: ctx.existsCurrentLockfile,
        existsNonEmptyWantedLockfile: ctx.existsNonEmptyWantedLockfile,
        lockfileDir: ctx.lockfileDir,
        storeDir: ctx.storeDir,
        registries: ctx.registries,
      })
    }
  }

  // Check if any custom resolvers want to force resolution for specific dependencies
  // Skip this check when not saving the lockfile (e.g., during deploy) since there's no point
  // in forcing re-resolution if we're not going to persist the results
  let forceResolutionFromHook = false
  const shouldCheckCustomResolverForceResolve =
    opts.hooks.customResolvers &&
    ctx.existsNonEmptyWantedLockfile &&
    !opts.frozenLockfile &&
    opts.saveLockfile
  if (shouldCheckCustomResolverForceResolve) {
    forceResolutionFromHook = await checkCustomResolverForceResolve(
      opts.hooks.customResolvers!,
      ctx.wantedLockfile
    )
  }

  const pruneVirtualStore = !opts.enableGlobalVirtualStore && (ctx.modulesFile?.prunedAt && opts.modulesCacheMaxAge > 0
    ? cacheExpired(ctx.modulesFile.prunedAt, opts.modulesCacheMaxAge)
    : true
  )

  if (!maybeOpts.ignorePackageManifest) {
    for (const { manifest, rootDir } of Object.values(ctx.projects)) {
      if (!manifest) {
        throw new Error(`No package.json found in "${rootDir}"`)
      }
    }
  }

  const result = await settleInstall(_install(), verifyLockfilePromise)

  // @ts-expect-error
  if (global['verifiedFileIntegrity'] > 1000) {
    // @ts-expect-error
    globalInfo(`The integrity of ${global['verifiedFileIntegrity']} files was checked. This might have caused installation to take longer.`)
  }

  if (opts.mergeGitBranchLockfiles) {
    await cleanGitBranchLockfiles(ctx.lockfileDir)
  }

  let ignoredBuilds = result.ignoredBuilds
  if (!opts.ignoreScripts && ignoredBuilds?.size) {
    ignoredBuilds = await runUnignoredDependencyBuilds(opts, ignoredBuilds, ctx.wantedLockfile, allowBuild)
  }
  let revokedBuilds = false
  // Detect packages whose build approval was revoked between the previous
  // and current install. A package is considered revoked when it was
  // previously allowed (true) but is now undecided (undefined). Packages
  // explicitly denied (false) are not added to ignoredBuilds, consistent
  // with how buildModules treats them.
  if (
    ctx.modulesFile?.allowBuilds &&
    ctx.wantedLockfile.packages &&
    Object.values(ctx.modulesFile.allowBuilds).some((v) => v === true)
  ) {
    const oldAllowBuild = createAllowBuildFunction({ allowBuilds: ctx.modulesFile.allowBuilds })
    if (oldAllowBuild) {
      for (const depPath of Object.keys(ctx.wantedLockfile.packages) as DepPath[]) {
        if (ignoredBuilds?.has(depPath)) continue
        // The old policy is evaluated with identity trust overridden so that
        // package-name approvals count as they did when they were granted,
        // even for git/tarball artifacts that the current policy no longer
        // approves by name.
        if (oldAllowBuild(depPath, { trustPackageIdentity: true }) !== true) continue
        if (allowBuild?.(depPath) === undefined) {
          ignoredBuilds ??= new Set()
          ignoredBuilds.add(depPath)
          revokedBuilds = true
        }
      }
    }
  }
  if (revokedBuilds && !opts.lockfileOnly && opts.enableModulesDir) {
    // The install path already wrote .modules.yaml with the current
    // install's state, but it captured ignoredBuilds before the revocation
    // scan above added to it. Re-read the manifest from disk so we only
    // update ignoredBuilds and don't clobber fields (hoistedDependencies,
    // pendingBuilds, etc.) the install just wrote. The current computed
    // set is authoritative — runUnignoredDependencyBuilds may have removed
    // entries (for packages it successfully rebuilt) that the on-disk
    // manifest still records, and those must not be re-introduced.
    const writtenManifest = await readModulesManifest(ctx.rootModulesDir)
    if (writtenManifest) {
      // writeModulesManifest converts ignoredBuilds to an array before
      // serializing, so a Set is fine here.
      writtenManifest.ignoredBuilds = ignoredBuilds
      await writeModulesManifest(ctx.rootModulesDir, writtenManifest as StrictModules)
    }
  }
  ignoredScriptsLogger.debug({
    packageNames: ignoredBuilds ? dedupePackageNamesFromIgnoredBuilds(ignoredBuilds) : [],
  })

  detachReporter()

  return {
    updatedCatalogs: result.updatedCatalogs,
    updatedProjects: result.updatedProjects,
    stats: result.stats ?? { added: 0, removed: 0, linkedToRoot: 0 },
    depsRequiringBuild: result.depsRequiringBuild,
    ignoredBuilds,
    resolutionPolicyViolations: result.resolutionPolicyViolations ?? [],
    dryRunResult: result.dryRunResult,
  }

  interface InnerInstallResult {
    readonly updatedCatalogs?: Catalogs
    readonly updatedProjects: UpdatedProject[]
    readonly stats?: InstallationResultStats
    readonly depsRequiringBuild?: DepPath[]
    readonly ignoredBuilds: IgnoredBuilds | undefined
    readonly dryRunResult?: DryRunInstallResult
    readonly resolutionPolicyViolations?: ResolutionPolicyViolation[]
  }

  // Reconcile the install with the lockfile verification that runs alongside
  // it. The verification verdict is awaited first so it takes precedence and
  // aborts as soon as it fails, even while the install is still in flight —
  // matching the original sequencing where verification gated the install, so
  // a rejected lockfile surfaces its own error rather than whatever the
  // concurrent install happened to throw. Only once verification passes is the
  // install's result (or error) surfaced. detachReporter mirrors the success
  // path's cleanup so a long-lived process doesn't leak the stream listener on
  // a rejected install.
  async function settleInstall (
    install: Promise<InnerInstallResult>,
    verification: Promise<void> | undefined
  ): Promise<InnerInstallResult> {
    if (verification == null) return install
    // Handle the install's eventual rejection up front so a fail-fast
    // verification throw below doesn't leave the still-running install
    // unhandled.
    install.catch(() => {})
    try {
      await verification
      return await install
    } catch (err) {
      detachReporter()
      throw err
    }
  }

  async function _install (): Promise<InnerInstallResult> {
    const scriptsOpts: RunLifecycleHooksConcurrentlyOptions = {
      extraBinPaths: opts.extraBinPaths,
      extraNodePaths: ctx.extraNodePaths,
      extraEnv: opts.extraEnv,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      userAgent: opts.userAgent,
      resolveSymlinksInInjectedDirs: opts.resolveSymlinksInInjectedDirs,
      scriptsPrependNodePath: opts.scriptsPrependNodePath,
      scriptShell: opts.scriptShell,
      shellEmulator: opts.shellEmulator,
      stdio: opts.ownLifecycleHooksStdio,
      storeController: opts.storeController,
      unsafePerm: opts.unsafePerm || false,
    }

    if (!opts.ignoreScripts && !opts.ignorePackageManifest && rootProjectManifest?.scripts?.[DEV_PREINSTALL]) {
      await runLifecycleHook(
        DEV_PREINSTALL,
        rootProjectManifest,
        {
          ...scriptsOpts,
          depPath: opts.lockfileDir,
          pkgRoot: opts.lockfileDir,
          rootModulesDir: ctx.rootModulesDir,
        }
      )
    }
    const packageExtensionsChecksum = hashObjectNullableWithPrefix(opts.packageExtensions)
    const pnpmfileChecksum = await opts.hooks.calculatePnpmfileChecksum?.()
    const patchedDependencies = opts.ignorePackageManifest
      ? ctx.wantedLockfile.patchedDependencies
      : (opts.patchedDependencies ? await calcPatchHashes(opts.patchedDependencies) : {})
    const patchGroupInput = opts.patchedDependencies
      ? Object.fromEntries(
        Object.entries(patchedDependencies ?? {}).map(([key, hash]) => {
          let patchFilePath = opts.patchedDependencies![key]
            ? path.resolve(opts.lockfileDir, opts.patchedDependencies![key])
            : undefined
          if (!patchFilePath) {
            const lastAt = key.lastIndexOf('@')
            const pkgName = lastAt > 0 ? key.slice(0, lastAt) : key
            if (opts.patchedDependencies![pkgName]) {
              patchFilePath = path.resolve(opts.lockfileDir, opts.patchedDependencies![pkgName])
            }
          }
          return [key, { hash, patchFilePath }]
        })
      )
      : patchedDependencies
    const patchGroups = patchGroupInput ? groupPatchedDependencies(patchGroupInput) : undefined
    const frozenLockfile = opts.frozenLockfile ||
      opts.frozenLockfileIfExists && ctx.existsNonEmptyWantedLockfile
    let outdatedLockfileSettings = false
    const overridesMap = createOverridesMapFromParsed(opts.parsedOverrides)
    if (!opts.ignorePackageManifest) {
      const outdatedLockfileSettingName = getOutdatedLockfileSetting(ctx.wantedLockfile, {
        autoInstallPeers: opts.autoInstallPeers,
        catalogs: opts.catalogs,
        dedupePeers: opts.dedupePeers || undefined,
        injectWorkspacePackages: opts.injectWorkspacePackages,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        peersSuffixMaxLength: opts.peersSuffixMaxLength,
        overrides: overridesMap,
        ignoredOptionalDependencies: opts.ignoredOptionalDependencies?.sort(),
        packageExtensionsChecksum,
        patchedDependencies,
        pnpmfileChecksum,
      })
      outdatedLockfileSettings = outdatedLockfileSettingName != null
      if (frozenLockfile && outdatedLockfileSettings) {
        throw new LockfileConfigMismatchError(outdatedLockfileSettingName!)
      }
    }
    const _isWantedDepBareSpecifierSame = isWantedDepBareSpecifierSame.bind(null, ctx.wantedLockfile.catalogs, opts.catalogs)
    const upToDateLockfileMajorVersion = ctx.wantedLockfile.lockfileVersion.toString().startsWith(`${LOCKFILE_MAJOR_VERSION}.`)
    let needsFullResolution = outdatedLockfileSettings ||
      opts.fixLockfile ||
      opts.updateChecksums ||
      !upToDateLockfileMajorVersion ||
      opts.forceFullResolution ||
      forceResolutionFromHook
    if (needsFullResolution) {
      ctx.wantedLockfile.settings = {
        autoInstallPeers: opts.autoInstallPeers,
        dedupePeers: opts.dedupePeers || undefined,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        peersSuffixMaxLength: opts.peersSuffixMaxLength,
        injectWorkspacePackages: opts.injectWorkspacePackages,
      }
      ctx.wantedLockfile.overrides = overridesMap
      ctx.wantedLockfile.packageExtensionsChecksum = packageExtensionsChecksum
      ctx.wantedLockfile.ignoredOptionalDependencies = opts.ignoredOptionalDependencies
      ctx.wantedLockfile.pnpmfileChecksum = pnpmfileChecksum
      ctx.wantedLockfile.patchedDependencies = patchedDependencies
    } else if (!frozenLockfile) {
      ctx.wantedLockfile.settings = {
        autoInstallPeers: opts.autoInstallPeers,
        dedupePeers: opts.dedupePeers || undefined,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        peersSuffixMaxLength: opts.peersSuffixMaxLength,
        injectWorkspacePackages: opts.injectWorkspacePackages,
      }
    }

    const frozenInstallResult = await tryFrozenInstall({
      frozenLockfile,
      needsFullResolution,
      patchGroups,
      upToDateLockfileMajorVersion,
    })
    if (frozenInstallResult !== null) {
      if ('needsFullResolution' in frozenInstallResult) {
        needsFullResolution = frozenInstallResult.needsFullResolution
      } else {
        return frozenInstallResult
      }
    }

    const projectsToInstall = [] as ImporterToUpdate[]

    let preferredSpecs: Record<string, string> | null = null

    // TODO: make it concurrent
    /* eslint-disable no-await-in-loop */
    for (const project of projects) {
      const projectOpts = {
        ...project,
        ...ctx.projects[project.rootDir],
      }
      switch (project.mutation) {
        case 'uninstallSome':
          projectsToInstall.push({
            pruneDirectDependencies: false,
            ...projectOpts,
            removePackages: project.dependencyNames,
            updatePackageManifest: true,
            wantedDependencies: [],
          })
          break
        case 'install': {
          await installCase({
            ...projectOpts,
            updatePackageManifest: (projectOpts as InstallDepsMutation).updatePackageManifest ?? (projectOpts as InstallDepsMutation).update!,
          })
          break
        }
        case 'installSome': {
          await installSome({
            ...projectOpts as InstallSomeProject,
            updatePackageManifest: (projectOpts as InstallSomeDepsMutation).updatePackageManifest !== false,
          })
          break
        }
      }
    }
    /* eslint-enable no-await-in-loop */

    type InstallCaseProject = Pick<ImporterToUpdate,
    | 'binsDir'
    | 'buildIndex'
    | 'id'
    | 'manifest'
    | 'modulesDir'
    | 'mutation'
    | 'rootDir'
    | 'updatePackageManifest'
    >

    async function installCase (project: InstallCaseProject) {
      const wantedDependencies = getWantedDependencies(project.manifest, {
        autoInstallPeers: opts.autoInstallPeers,
        includeDirect: opts.includeDirect,
      })
        .map((wantedDependency) => ({ ...wantedDependency, updateSpec: true }))
      if (opts.packageVulnerabilityAudit) {
        for (const dep of wantedDependencies) {
          let specifier: string | undefined = dep.bareSpecifier
          const catalogName = specifier ? parseCatalogProtocol(specifier) : null
          if (catalogName != null) {
            const catalogResult = resolveFromCatalog(opts.catalogs, { alias: dep.alias, bareSpecifier: specifier! })
            specifier = matchCatalogResolveResult(catalogResult, pickCatalogSpecifier)
          }
          const validVersion = semver.valid(specifier)
          // Only proceed if the specifier is a pinned version, not a range
          if (!validVersion) continue
          if (opts.packageVulnerabilityAudit.isVulnerable(dep.alias, validVersion)) {
            // If the current version is pinned and vulnerable, expand the specifier to a range
            // that will allow updating to a non-vulnerable, semver-compatible version, if available.
            if (catalogName != null && opts.catalogs?.[catalogName]) {
              // If a catalog is used, update the catalog entry so the resolver can find a
              // non-vulnerable version. The package.json keeps "catalog:" and the workspace manifest
              // gets updated.
              opts.catalogs = {
                ...opts.catalogs,
                [catalogName]: {
                  ...opts.catalogs[catalogName],
                  [dep.alias]: '^' + validVersion,
                },
              }
              // Set prevSpecifier to the original catalog specifier so the resolver
              // preserves the original pinning style (i.e. pinned stays pinned).
              dep.prevSpecifier = specifier
            } else {
              // If no catalog is used, we directly update the specifier.
              dep.bareSpecifier = '^' + validVersion
            }
          }
        }
      }

      if (ctx.wantedLockfile?.importers) {
        forgetResolutionsOfPrevWantedDeps(ctx.wantedLockfile.importers[project.id], wantedDependencies, _isWantedDepBareSpecifierSame)
      }
      if (opts.ignoreScripts && project.manifest?.scripts &&
        (project.manifest.scripts.preinstall != null ||
          project.manifest.scripts.install != null ||
          project.manifest.scripts.postinstall != null ||
          project.manifest.scripts.prepare)
      ) {
        ctx.pendingBuilds.push(project.id)
      }

      projectsToInstall.push({
        pruneDirectDependencies: false,
        ...project,
        wantedDependencies,
      } as ImporterToUpdate)
    }

    type InstallSomeProject = Pick<ImporterToUpdate,
    | 'binsDir'
    | 'buildIndex'
    | 'id'
    | 'manifest'
    | 'modulesDir'
    | 'mutation'
    | 'rootDir'
    | 'updatePackageManifest'
    > & Pick<InstallSomeDepsMutation,
    | 'allowNew'
    | 'dependencySelectors'
    | 'targetDependenciesField'
    | 'update'
    >

    async function installSome (project: InstallSomeProject) {
      const currentBareSpecifiers = opts.ignoreCurrentSpecifiers
        ? {}
        : getAllDependenciesFromManifest(project.manifest, { autoInstallPeers: opts.autoInstallPeers })
      const optionalDependencies = project.targetDependenciesField ? {} : project.manifest.optionalDependencies ?? {}
      const devDependencies = project.targetDependenciesField ? {} : project.manifest.devDependencies ?? {}
      if (preferredSpecs == null) {
        const manifests = []
        for (const versions of ctx.workspacePackages.values()) {
          for (const { manifest } of versions.values()) {
            manifests.push(manifest)
          }
        }
        preferredSpecs = getAllUniqueSpecs(manifests)
      }
      const wantedDeps = parseWantedDependencies(project.dependencySelectors, {
        allowNew: project.allowNew !== false,
        currentBareSpecifiers,
        defaultTag: opts.tag,
        dev: project.targetDependenciesField === 'devDependencies',
        devDependencies,
        optional: project.targetDependenciesField === 'optionalDependencies',
        optionalDependencies,
        updateWorkspaceDependencies: project.update,
        preferredSpecs,
        saveCatalogName: opts.saveCatalogName,
        overrides: opts.overrides,
        onOverrideApplied: (selector) => opts.appliedOverrides.add(selector),
        defaultCatalog: opts.catalogs?.default,
      })

      if (opts.catalogMode !== 'manual') {
        for (const wantedDep of wantedDeps) {
          // A `runtime:` specifier (e.g. node from `devEngines.runtime` or
          // `pnpm runtime set`) round-trips to `devEngines.runtime` through the
          // manifest writer, which only recognizes the `runtime:` protocol.
          // Promoting it into a catalog rewrites the entry to `catalog:`, which
          // breaks that round-trip and strands it in `devDependencies`.
          if (wantedDep.bareSpecifier?.startsWith('runtime:')) continue
          const perDepCatalogName = getPerDepCatalogName(wantedDep, opts.saveCatalogName)
          const catalogBareSpecifier = `catalog:${perDepCatalogName === 'default' ? '' : perDepCatalogName}`
          const catalog = resolveFromCatalog(opts.catalogs, { ...wantedDep, bareSpecifier: catalogBareSpecifier })
          const catalogDepSpecifier = matchCatalogResolveResult(catalog, pickCatalogSpecifier)

          if (
            !catalogDepSpecifier ||
            wantedDep.bareSpecifier === catalogBareSpecifier ||
            semver.valid(wantedDep.bareSpecifier) &&
            semver.valid(catalogDepSpecifier) &&
            semver.eq(wantedDep.bareSpecifier, catalogDepSpecifier)
          ) {
            wantedDep.saveCatalogName = perDepCatalogName
            continue
          }

          switch (opts.catalogMode) {
            case 'strict':
              throw new CatalogVersionMismatchError({ catalogDep: `${wantedDep.alias}@${catalogDepSpecifier}`, wantedDep: `${wantedDep.alias}@${wantedDep.bareSpecifier}` })

            case 'prefer':
              logger.warn({
                message: `Catalog version mismatch for "${wantedDep.alias}": using direct version "${wantedDep.bareSpecifier}" instead of catalog version "${catalogDepSpecifier}".`,
                prefix: opts.lockfileDir,
              })
          }
        }
      }

      projectsToInstall.push({
        pruneDirectDependencies: false,
        ...project,
        wantedDependencies: wantedDeps.map(wantedDep => ({ ...wantedDep, isNew: !currentBareSpecifiers[wantedDep.alias], updateSpec: true })),
      } as ImporterToUpdate)
    }

    // Unfortunately, the private lockfile may differ from the public one.
    // A user might run named installations on a project that has a pnpm-lock.yaml file before running a noop install
    const makePartialCurrentLockfile = !installsOnly && (
      ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile ||
      !ctx.currentLockfileIsUpToDate
    )
    const result = await installInContext(projectsToInstall, ctx, {
      ...opts,
      allowBuild,
      currentLockfileIsUpToDate: !ctx.existsNonEmptyWantedLockfile || ctx.currentLockfileIsUpToDate,
      makePartialCurrentLockfile,
      needsFullResolution,
      pruneVirtualStore,
      scriptsOpts,
      updateLockfileMinorVersion: true,
      patchedDependencies: patchGroups,
      verifyLockfile,
    })

    return {
      updatedCatalogs: result.updatedCatalogs,
      updatedProjects: result.projects,
      stats: result.stats,
      depsRequiringBuild: result.depsRequiringBuild,
      ignoredBuilds: result.ignoredBuilds,
      resolutionPolicyViolations: result.resolutionPolicyViolations,
      dryRunResult: result.dryRunResult,
    }
  }

  /**
   * Attempt to perform a "frozen install".
   *
   * A "frozen install" will be performed if:
   *
   *   1. The --frozen-lockfile flag was explicitly specified or evaluates to
   *      true based on conditions like running on CI.
   *   2. No workspace modifications have been made that would invalidate the
   *      pnpm-lock.yaml file. In other words, the pnpm-lock.yaml file is
   *      known to be "up-to-date".
   *
   * A frozen install is significantly faster since the pnpm-lock.yaml file
   * can treated as immutable, skipping expensive lookups to acquire new
   * dependencies. For this reason, a frozen install should be performed even
   * if --frozen-lockfile wasn't explicitly specified. This allows users to
   * benefit from the increased performance of a frozen install automatically.
   *
   * If a frozen install is not possible, this function will return null.
   * This indicates a standard mutable install needs to be performed.
   *
   * Note this function may update the pnpm-lock.yaml file if the lockfile was
   * on a different major version, needs to be merged due to git conflicts,
   * etc. These changes update the format of the pnpm-lock.yaml file, but do
   * not change recorded dependency resolutions.
   */
  async function tryFrozenInstall ({
    frozenLockfile,
    needsFullResolution,
    patchGroups,
    upToDateLockfileMajorVersion,
  }: {
    frozenLockfile: boolean
    needsFullResolution: boolean
    patchGroups?: PatchGroupRecord
    upToDateLockfileMajorVersion: boolean
  }): Promise<InnerInstallResult | { needsFullResolution: boolean } | null> {
    const isFrozenInstallPossible =
      // A frozen install is never possible when any of these are true:
      !ctx.lockfileHadConflicts &&
      !opts.fixLockfile &&
      !opts.dedupe &&

      // A check-only install (`lockfileCheck`, used by `--dry-run` and
      // `dedupe --check`) must always run a full resolution so the wanted
      // lockfile can be compared, and must never materialize anything. The
      // frozen path would skip resolution and/or perform a real install.
      !isCheckOnlyInstall(opts) &&

      installsOnly &&
      (
        // If the user explicitly requested a frozen lockfile install, attempt
        // to perform one. An error will be thrown if updates are required.
        frozenLockfile ||

        // Otherwise, check if a frozen-like install is possible for
        // performance. This will be the case if all projects are up-to-date.
        opts.ignorePackageManifest ||
        !needsFullResolution &&
        opts.preferFrozenLockfile &&
        (!opts.pruneLockfileImporters || Object.keys(ctx.wantedLockfile.importers).length === Object.keys(ctx.projects).length) &&
        !isEmptyLockfile(ctx.wantedLockfile) &&
        ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION &&
        await allProjectsAreUpToDate(Object.values(ctx.projects), {
          catalogs: opts.catalogs,
          autoInstallPeers: opts.autoInstallPeers,
          excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
          linkWorkspacePackages: opts.linkWorkspacePackagesDepth >= 0,
          wantedLockfile: ctx.wantedLockfile,
          workspacePackages: ctx.workspacePackages,
          lockfileDir: opts.lockfileDir,
        })
      )

    if (!isFrozenInstallPossible) {
      return null
    }

    if (needsFullResolution) {
      throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
        'Cannot perform a frozen installation because the version of the lockfile is incompatible with this version of pnpm',
        {
          hint: `Try either:
1. Aligning the version of pnpm that generated the lockfile with the version that installs from it, or
2. Migrating the lockfile so that it is compatible with the newer version of pnpm, or
3. Using "pnpm install --no-frozen-lockfile".
Note that in CI environments, this setting is enabled by default.`,
        }
      )
    }
    if (!opts.ignorePackageManifest) {
      // `--frozen-lockfile` (the CI default) means "fail if pnpm-lock.yaml is
      // out of sync." Treat its absence as a sync failure even when the
      // synthesized snapshot from node_modules/.pnpm/lock.yaml would satisfy
      // the manifest — the developer needs to commit the regenerated file.
      if (frozenLockfile && !ctx.existsWantedLockfile &&
        Object.values(ctx.projects).some((project) => pkgHasDependencies(project.manifest))) {
        throw new PnpmError('NO_LOCKFILE',
          `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is absent`, {
            hint: 'Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"',
          })
      }
      const _satisfiesPackageManifest = satisfiesPackageManifest.bind(null, {
        autoInstallPeers: opts.autoInstallPeers,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      })
      for (const { id, manifest, rootDir } of Object.values(ctx.projects)) {
        const { satisfies, detailedReason } = _satisfiesPackageManifest(ctx.wantedLockfile.importers[id], manifest)
        if (!satisfies) {
          if (!ctx.existsWantedLockfile) {
            throw new PnpmError('NO_LOCKFILE',
              `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is absent`, {
                hint: 'Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"',
              })
          }

          throw new PnpmError('OUTDATED_LOCKFILE',
            `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ` +
            path.join('<ROOT>', path.relative(opts.lockfileDir, path.join(rootDir, 'package.json'))), {
              hint: `Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"

  Failure reason:
  ${detailedReason ?? ''}`,
            })
        }
      }
    }
    if (opts.lockfileOnly) {
      // The lockfile will only be changed if the workspace will have new projects with no dependencies.
      await writeWantedLockfile(ctx.lockfileDir, ctx.wantedLockfile)
      return {
        updatedProjects: projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir]),
        ignoredBuilds: undefined,
      }
    }
    if (isEmptyLockfile(ctx.wantedLockfile)) {
      if (Object.values(ctx.projects).some((project) => pkgHasDependencies(project.manifest))) {
        throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
      }
      return null
    }

    if (maybeOpts.ignorePackageManifest) {
      logger.info({ message: 'Importing packages to virtual store', prefix: opts.lockfileDir })
    } else {
      logger.info({ message: 'Lockfile is up to date, resolution step is skipped', prefix: opts.lockfileDir })
    }
    if (opts.runPacquet != null && opts.useLockfile && !opts.useGitBranchLockfile && !opts.mergeGitBranchLockfiles && !isCheckOnlyInstall(opts) && opts.enableModulesDir) {
      try {
        await opts.runPacquet.run()
      } catch (err) {
        // Same reasoning as the verifyLockfileResolutions catch above: this
        // is the user-facing failure path, so detach the reporter listener
        // before rethrowing so long-lived processes don't leak it.
        detachReporter()
        throw err
      }
      return {
        updatedProjects: projects.map((mutatedProject) => {
          const project = ctx.projects[mutatedProject.rootDir]
          return {
            ...project,
            manifest: project.originalManifest ?? project.manifest,
          }
        }),
        ignoredBuilds: undefined,
      }
    }
    try {
      const { stats, ignoredBuilds } = await headlessInstall({
        ...ctx,
        ...opts,
        currentEngine: {
          nodeVersion: opts.nodeVersion,
          pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
        },
        currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
        patchedDependencies: patchGroups,
        selectedProjectDirs: projects.map((project) => project.rootDir),
        allProjects: ctx.projects,
        prunedAt: ctx.modulesFile?.prunedAt,
        pruneVirtualStore,
        wantedLockfile: maybeOpts.ignorePackageManifest ? undefined : ctx.wantedLockfile,
        useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
        verifyLockfile,
      })
      if (
        opts.useLockfile && opts.saveLockfile && opts.mergeGitBranchLockfiles ||
        !upToDateLockfileMajorVersion && !opts.frozenLockfile
      ) {
        const currentLockfileDir = path.join(ctx.rootModulesDir, '.pnpm')
        await writeLockfiles({
          currentLockfile: ctx.currentLockfile,
          currentLockfileDir,
          wantedLockfile: ctx.wantedLockfile,
          wantedLockfileDir: ctx.lockfileDir,
          useGitBranchLockfile: opts.useGitBranchLockfile,
          mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
        })
      }
      return {
        updatedProjects: projects.map((mutatedProject) => {
          const project = ctx.projects[mutatedProject.rootDir]
          return {
            ...project,
            manifest: project.originalManifest ?? project.manifest,
          }
        }),
        stats,
        ignoredBuilds,
      }
    } catch (error: any) { // eslint-disable-line
      const isIntegrityError = BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)
      if (
        frozenLockfile ||
        (
          error.code !== 'ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY' &&
          !isIntegrityError
        ) ||
        (!ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile) ||
        (isIntegrityError && !opts.updateChecksums)
      ) throw error
      // A broken lockfile may be caused by a badly resolved Git conflict
      logger.warn({
        error,
        message: error.message,
        prefix: ctx.lockfileDir,
      })
      logger.error(new PnpmError(error.code, 'The lockfile is broken! Resolution step will be performed to fix it.'))
      return { needsFullResolution }
    }
  }
}

async function runUnignoredDependencyBuilds (
  opts: StrictInstallOptions,
  previousIgnoredBuilds: IgnoredBuilds,
  currentLockfile: LockfileObject,
  allowBuild?: AllowBuild
): Promise<Set<DepPath>> {
  if (!allowBuild) {
    return previousIgnoredBuilds
  }
  const pkgsToBuild: string[] = []
  for (const ignoredPkg of previousIgnoredBuilds) {
    if (currentLockfile.packages?.[ignoredPkg] == null) continue
    if (allowBuild(ignoredPkg) === true) {
      // Package is explicitly allowed - rebuild it
      pkgsToBuild.push(dp.getPkgIdWithPatchHash(ignoredPkg))
    }
  }
  if (pkgsToBuild.length) {
    return (await buildSelectedPkgs(opts.allProjects, pkgsToBuild, {
      ...opts,
      reporter: undefined, // We don't want to attach the reporter again, it was already attached.
      rootProjectManifestDir: opts.lockfileDir,
    })).ignoredBuilds ?? previousIgnoredBuilds
  }
  return previousIgnoredBuilds
}

function cacheExpired (prunedAt: string, maxAgeInMinutes: number): boolean {
  return ((Date.now() - new Date(prunedAt).valueOf()) / (1000 * 60)) > maxAgeInMinutes
}

function pkgHasDependencies (manifest: ProjectManifest): boolean {
  return Boolean(
    (Object.keys(manifest.dependencies ?? {}).length > 0) ||
    Object.keys(manifest.devDependencies ?? {}).length ||
    Object.keys(manifest.optionalDependencies ?? {}).length
  )
}

// If the specifier is new, the old resolution probably does not satisfy it anymore.
// By removing these resolutions we ensure that they are resolved again using the new specs.
function forgetResolutionsOfPrevWantedDeps (
  importer: ProjectSnapshot,
  wantedDeps: WantedDependency[],
  isWantedDepBareSpecifierSame: (alias: string, prevBareSpecifier: string | undefined, nextBareSpecifier: string) => boolean
): void {
  if (!importer.specifiers) return
  importer.dependencies = importer.dependencies ?? {}
  importer.devDependencies = importer.devDependencies ?? {}
  importer.optionalDependencies = importer.optionalDependencies ?? {}
  for (const { alias, bareSpecifier } of wantedDeps) {
    if (alias && !isWantedDepBareSpecifierSame(alias, importer.specifiers[alias], bareSpecifier)) {
      if (!importer.dependencies[alias]?.startsWith('link:')) {
        delete importer.dependencies[alias]
      }
      delete importer.devDependencies[alias]
      delete importer.optionalDependencies[alias]
    }
  }
}

function forgetResolutionsOfAllPrevWantedDeps (wantedLockfile: LockfileObject): void {
  // Similar to the forgetResolutionsOfPrevWantedDeps function above, we can
  // delete existing resolutions in importers to make sure they're resolved
  // again.
  if ((wantedLockfile.importers != null) && !isEmpty(wantedLockfile.importers)) {
    wantedLockfile.importers = mapValues(
      ({ dependencies: _dependencies, devDependencies: _devDependencies, optionalDependencies: _optionalDependencies, ...rest }) => rest,
      wantedLockfile.importers)
  }

  // The resolveDependencies function looks at previous PackageSnapshot
  // dependencies/optionalDependencies blocks and merges them with new resolved
  // deps. Clear the previous PackageSnapshot fields so the newly resolved deps
  // are always used.
  if ((wantedLockfile.packages != null) && !isEmpty(wantedLockfile.packages)) {
    wantedLockfile.packages = mapValues(
      ({ dependencies: _dependencies, optionalDependencies: _optionalDependencies, ...rest }) => rest,
      wantedLockfile.packages)
  }

  // Also clear the resolutions in catalogs so they're re-resolved and deduped.
  if ((wantedLockfile.catalogs != null) && !isEmpty(wantedLockfile.catalogs)) {
    wantedLockfile.catalogs = undefined
  }
}

/**
 * Check if a wanted bareSpecifier is the same.
 *
 * It would be different if the user modified a dependency in package.json or a
 * catalog entry in pnpm-workspace.yaml. This is normally a simple check to see
 * if the specifier strings match, but catalogs make this more involved since we
 * also have to check if the catalog config in pnpm-workspace.yaml is the same.
 */
function isWantedDepBareSpecifierSame (
  prevCatalogs: CatalogSnapshots | undefined,
  catalogsConfig: Catalogs | undefined,
  alias: string,
  prevBareSpecifier: string | undefined,
  nextBareSpecifier: string
): boolean {
  if (prevBareSpecifier !== nextBareSpecifier) {
    return false
  }

  // When pnpm catalogs are used, the specifiers can be the same (e.g.
  // "catalog:default"), but the wanted versions for the dependency can be
  // different after resolution if the catalog config was just edited.
  const catalogName = parseCatalogProtocol(prevBareSpecifier)

  // If there's no catalog name, the catalog protocol was not used and we
  // can assume the bareSpecifier is the same since prevBareSpecifier and nextBareSpecifier match.
  if (catalogName === null) {
    return true
  }

  const prevCatalogEntrySpec = prevCatalogs?.[catalogName]?.[alias]?.specifier
  const nextCatalogEntrySpec = catalogsConfig?.[catalogName]?.[alias]

  return prevCatalogEntrySpec === nextCatalogEntrySpec
}

/**
 * Determines the catalog name for a dependency during installSome.
 *
 * If the dependency's previous specifier already uses a named catalog
 * (e.g. "catalog:foo"), that catalog name takes priority over the global
 * saveCatalogName option. This ensures that interactive updates and
 * `--latest` upgrades preserve the per-dependency catalog group.
 */
function getPerDepCatalogName (
  wantedDep: { prevSpecifier?: string },
  globalSaveCatalogName: string | undefined
): string {
  if (wantedDep.prevSpecifier) {
    const catalogFromPrev = parseCatalogProtocol(wantedDep.prevSpecifier)
    if (catalogFromPrev != null) {
      return catalogFromPrev
    }
  }
  return globalSaveCatalogName ?? 'default'
}

export async function addDependenciesToPackage (
  manifest: ProjectManifest,
  dependencySelectors: string[],
  opts: Omit<InstallOptions, 'allProjects'> & {
    bin?: string
    allowNew?: boolean
    peer?: boolean
    pinnedVersion?: 'major' | 'minor' | 'patch'
    targetDependenciesField?: DependenciesField
  } & InstallMutationOptions
): Promise<InstallResult> {
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir
  const { updatedCatalogs, updatedProjects: projects, ignoredBuilds, resolutionPolicyViolations } = await mutateModules(
    [
      {
        allowNew: opts.allowNew,
        dependencySelectors,
        mutation: 'installSome',
        peer: opts.peer,
        pinnedVersion: opts.pinnedVersion,
        rootDir,
        targetDependenciesField: opts.targetDependenciesField,
        update: opts.update,
        updateMatching: opts.updateMatching,
        updatePackageManifest: opts.updatePackageManifest,
        updateToLatest: opts.updateToLatest,
      },
    ],
    {
      ...opts,
      lockfileDir: opts.lockfileDir ?? opts.dir,
      allProjects: [
        {
          buildIndex: 0,
          binsDir: opts.bin,
          manifest,
          rootDir,
        },
      ],
    })
  return { updatedCatalogs, updatedManifest: projects[0].manifest, ignoredBuilds, resolutionPolicyViolations }
}

export type ImporterToUpdate = {
  buildIndex: number
  binsDir: string
  id: ProjectId
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  modulesDir: string
  rootDir: ProjectRootDir
  pruneDirectDependencies: boolean
  removePackages?: string[]
  updatePackageManifest: boolean
  wantedDependencies: Array<WantedDependency & { isNew?: boolean, updateSpec?: boolean }>
} & DependenciesMutation

export interface UpdatedProject {
  originalManifest?: ProjectManifest
  manifest: ProjectManifest
  peerDependencyIssues?: PeerDependencyIssues
  rootDir: ProjectRootDir
}

/**
 * The before/after wanted lockfiles a `dryRun` install resolved without
 * writing. The caller diffs them to report what a real install would change.
 */
export interface DryRunInstallResult {
  originalLockfile: LockfileObject
  wantedLockfile: LockfileObject
}

/**
 * A "check-only" install resolves fully but writes nothing: `dryRun`
 * (`pnpm install --dry-run`) and `lockfileCheck` (`pnpm dedupe --check`)
 * both take this path. The shared flag suppresses every write and forces a
 * full resolution (the frozen/headless fast paths are skipped) so the wanted
 * lockfile can always be compared.
 */
function isCheckOnlyInstall (opts: { lockfileCheck?: unknown, dryRun?: boolean }): boolean {
  return opts.lockfileCheck != null || opts.dryRun === true
}

interface InstallFunctionResult {
  updatedCatalogs?: Catalogs
  newLockfile: LockfileObject
  projects: UpdatedProject[]
  stats?: InstallationResultStats
  depsRequiringBuild: DepPath[]
  ignoredBuilds?: IgnoredBuilds
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  dryRunResult?: DryRunInstallResult
}

type InstallFunction = (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: Omit<StrictInstallOptions, 'patchedDependencies'> & {
    allowBuild?: AllowBuild
    patchedDependencies?: PatchGroupRecord
    makePartialCurrentLockfile: boolean
    needsFullResolution: boolean
    overrides?: Record<string, string>
    updateLockfileMinorVersion: boolean
    preferredVersions?: PreferredVersions
    pruneVirtualStore: boolean
    scriptsOpts: RunLifecycleHooksConcurrentlyOptions
    currentLockfileIsUpToDate: boolean
    hoistWorkspacePackages?: boolean
    verifyLockfile?: () => Promise<void>
  }
) => Promise<InstallFunctionResult>

const _installInContext: InstallFunction = async (projects, ctx, opts) => {
  // Aliasing for clarity in boolean expressions below. True for both
  // `--dry-run` and `dedupe --check`: resolve fully, write nothing.
  const isInstallationOnlyForLockfileCheck = isCheckOnlyInstall(opts)

  // The wanted lockfile is mutated during installation. To compare changes, a
  // deep copy before installation is needed. This copy should represent the
  // original wanted lockfile on disk as close as possible.
  //
  // This object can be quite large. Intentionally avoiding an expensive copy
  // unless this is a check-only install that needs the comparison.
  const originalLockfileForCheck = isInstallationOnlyForLockfileCheck
    ? clone(ctx.wantedLockfile)
    : null

  ctx.wantedLockfile.importers = ctx.wantedLockfile.importers || {}
  for (const { id } of projects) {
    if (!ctx.wantedLockfile.importers[id]) {
      ctx.wantedLockfile.importers[id] = { specifiers: {} }
    }
  }
  if (opts.pruneLockfileImporters) {
    const projectIds = new Set(projects.map(({ id }) => id))
    for (const wantedImporter of Object.keys(ctx.wantedLockfile.importers) as ProjectId[]) {
      if (!projectIds.has(wantedImporter)) {
        delete ctx.wantedLockfile.importers[wantedImporter]
      }
    }
  }

  await Promise.all(
    projects
      .map(async (project) => {
        if (project.mutation !== 'uninstallSome') return
        const _removeDeps = async (manifest: ProjectManifest) => removeDeps(manifest, project.dependencyNames, { prefix: project.rootDir, saveType: project.targetDependenciesField })
        project.manifest = await _removeDeps(project.manifest)
        if (project.originalManifest != null) {
          project.originalManifest = await _removeDeps(project.originalManifest)
        }
      })
  )

  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'resolution_started',
  })

  const update = projects.some((project) => (project as InstallMutationOptions).update)
  const preferredVersions = opts.preferredVersions ?? (
    !update
      ? getPreferredVersionsFromLockfileAndManifests(ctx.wantedLockfile.packages, Object.values(ctx.projects).map(({ manifest }) => manifest))
      : undefined
  )
  const forceFullResolution = ctx.wantedLockfile.lockfileVersion !== LOCKFILE_VERSION ||
    !opts.currentLockfileIsUpToDate ||
    opts.force ||
    opts.needsFullResolution ||
    ctx.lockfileHadConflicts ||
    opts.dedupePeerDependents

  // Ignore some fields when fixing lockfile, so these fields can be regenerated
  // and make sure it's up to date
  if (
    opts.fixLockfile &&
    (ctx.wantedLockfile.packages != null) &&
    !isEmpty(ctx.wantedLockfile.packages)
  ) {
    ctx.wantedLockfile.packages = mapValues(({ dependencies, optionalDependencies, resolution }) => ({
      // These fields are needed to avoid losing information of the locked dependencies if these fields are not broken
      // If these fields are broken, they will also be regenerated
      dependencies,
      optionalDependencies,
      resolution,
    }), ctx.wantedLockfile.packages)
  }

  if (opts.dedupe) {
    // Deleting recorded version resolutions from importers and packages. These
    // fields will be regenerated using the preferred versions computed above.
    //
    // This is a bit different from a "full resolution", which completely
    // ignores preferred versions from the lockfile.
    forgetResolutionsOfAllPrevWantedDeps(ctx.wantedLockfile)
  }

  let {
    dependenciesGraph,
    dependenciesByProjectId,
    linkedDependenciesByProjectId,
    updatedCatalogs,
    newLockfile,
    outdatedDependencies,
    peerDependencyIssuesByProjects,
    wantedToBeSkippedPackageIds,
    waitTillAllFetchingsFinish,
    resolutionPolicyViolations,
  } = await resolveDependencies(
    projects,
    {
      allowBuild: opts.allowBuild,
      allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
      allowUnusedPatches: opts.allowUnusedPatches,
      autoInstallPeers: opts.autoInstallPeers,
      autoInstallPeersFromHighestMatch: opts.autoInstallPeersFromHighestMatch,
      catalogs: opts.catalogs,
      currentLockfile: ctx.currentLockfile,
      defaultUpdateDepth: opts.depth,
      dedupeDirectDeps: opts.dedupeDirectDeps,
      dedupeInjectedDeps: opts.dedupeInjectedDeps,
      dedupePeerDependents: opts.dedupePeerDependents,
      dedupePeers: opts.dedupePeers,
      dryRun: opts.lockfileOnly,
      enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
      engineStrict: opts.engineStrict,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      force: opts.force,
      forceFullResolution,
      updateChecksums: opts.updateChecksums,
      ignoreScripts: opts.ignoreScripts,
      hooks: {
        readPackage: opts.readPackageHook,
      },
      linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? (opts.saveWorkspaceProtocol ? 0 : -1),
      lockfileDir: opts.lockfileDir,
      nodeVersion: opts.nodeVersion,
      pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
      preferWorkspacePackages: opts.preferWorkspacePackages,
      preferredVersions,
      preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
      registries: ctx.registries,
      namedRegistries: opts.namedRegistries,
      resolutionMode: opts.resolutionMode,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      storeController: opts.storeController,
      tag: opts.tag,
      globalVirtualStoreDir: opts.globalVirtualStoreDir,
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
      wantedLockfile: ctx.wantedLockfile,
      workspacePackages: ctx.workspacePackages,
      patchedDependencies: opts.patchedDependencies,
      lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
      resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
      supportedArchitectures: opts.supportedArchitectures,
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
      injectWorkspacePackages: opts.injectWorkspacePackages,
      minimumReleaseAge: opts.minimumReleaseAge,
      minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
      trustPolicy: opts.trustPolicy,
      trustPolicyExclude: opts.trustPolicyExclude,
      trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
      blockExoticSubdeps: opts.blockExoticSubdeps,
      allProjectIds: Object.values(ctx.projects).map((p) => p.id),
      handleResolutionPolicyViolations: opts.handleResolutionPolicyViolations,
    }
  )
  if (!opts.include.optionalDependencies || !opts.include.devDependencies || !opts.include.dependencies) {
    linkedDependenciesByProjectId = mapValues(
      (linkedDeps) => linkedDeps.filter((linkedDep) =>
        !(
          linkedDep.dev && !opts.include.devDependencies ||
          linkedDep.optional && !opts.include.optionalDependencies ||
          !linkedDep.dev && !linkedDep.optional && !opts.include.dependencies
        )),
      linkedDependenciesByProjectId ?? {}
    )
    for (const { id, manifest } of projects) {
      for (const [alias, depPath] of dependenciesByProjectId[id].entries()) {
        let include!: boolean
        const dep = dependenciesGraph[depPath]
        if (!dep) {
          include = false
        } else {
          const isDev = Boolean(manifest.devDependencies?.[dep.name])
          const isOptional = Boolean(manifest.optionalDependencies?.[dep.name])
          include = !(
            isDev && !opts.include.devDependencies ||
            isOptional && !opts.include.optionalDependencies ||
            !isDev && !isOptional && !opts.include.dependencies
          )
        }
        if (!include) {
          dependenciesByProjectId[id].delete(alias)
        }
      }
    }
  }
  if (opts.skipRuntimes) {
    // The lockfile filter (filterImporter) handles wantedLockfile-driven linking,
    // but the direct bin-linking path at the end of _installInContext iterates
    // dependenciesByProjectId and only filters by ctx.skipped. Add runtime
    // depPaths there so that path skips them too.
    for (const id of Object.keys(dependenciesByProjectId) as ProjectId[]) {
      for (const [alias, depPath] of dependenciesByProjectId[id].entries()) {
        if (depPath.includes('@runtime:')) {
          ctx.skipped.add(depPath)
          dependenciesByProjectId[id].delete(alias)
        }
      }
    }
  }

  // Same gate as the patches verifier (deps-resolver/index.ts): only check
  // when the whole lockfile was reanalyzed, otherwise the applied-override
  // set is incomplete (resolution short-circuited against the cache) and we
  // would warn about overrides that are actually in use. Emitted before the
  // 'resolution_done' stage so the reporter's buffer(resolutionDone$) captures it.
  if (
    opts.parsedOverrides.length &&
    (forceFullResolution || isEmpty(ctx.wantedLockfile.packages ?? {})) &&
    Object.keys(ctx.wantedLockfile.importers).length === projects.length
  ) {
    for (const override of opts.parsedOverrides) {
      if (!opts.appliedOverrides.has(override.selector)) {
        unusedOverrideLogger.debug({
          prefix: ctx.lockfileDir,
          selector: override.selector,
        })
      }
    }
  }

  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'resolution_done',
  })

  // `pnpm update` may bump catalog entries during resolution. Overrides that
  // reference a catalog (e.g. `overrides: { foo: 'catalog:' }`) were resolved
  // against the pre-update catalog when the install options were extended, so
  // re-resolve them against the updated catalog. Done before `afterAllResolved`
  // so that hook still sees (and can amend) the final overrides. Otherwise
  // lockfile `overrides` keeps pointing at the old version while `catalogs`
  // advances, and a later `--frozen-lockfile` install fails with
  // ERR_PNPM_LOCKFILE_CONFIG_MISMATCH.
  if (updatedCatalogs != null && opts.overrides != null && Object.keys(opts.overrides).length > 0) {
    newLockfile.overrides = createOverridesMapFromParsed(
      parseOverrides(opts.overrides, mergeCatalogs(opts.catalogs, updatedCatalogs))
    )
  }

  newLockfile = ((opts.hooks?.afterAllResolved) != null)
    ? await pipeWith(async (f, res) => f(await res), opts.hooks.afterAllResolved as any)(newLockfile) as LockfileObject // eslint-disable-line
    : newLockfile

  if (opts.updateLockfileMinorVersion) {
    newLockfile.lockfileVersion = LOCKFILE_VERSION
  }

  const depsStateCache: DepsStateCache = {}
  const lockfileOpts = {
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  }
  let stats: InstallationResultStats | undefined
  let ignoredBuilds: IgnoredBuilds | undefined
  const shouldWritePackageMap = opts.enableModulesDir !== false && opts.nodeLinker === 'isolated' && !opts.virtualStoreOnly
  if (!opts.lockfileOnly && !isInstallationOnlyForLockfileCheck && opts.enableModulesDir) {
    const result = await linkPackages(
      projects,
      dependenciesGraph,
      {
        allowBuild: opts.allowBuild,
        currentLockfile: ctx.currentLockfile,
        dedupeDirectDeps: opts.dedupeDirectDeps,
        dependenciesByProjectId,
        depsStateCache,
        disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
        enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
        extraNodePaths: ctx.extraNodePaths,
        force: opts.force,
        hoistedDependencies: ctx.hoistedDependencies,
        hoistedModulesDir: ctx.hoistedModulesDir,
        hoistPattern: ctx.hoistPattern,
        ignoreScripts: opts.ignoreScripts,
        include: opts.include,
        linkedDependenciesByProjectId,
        lockfileDir: opts.lockfileDir,
        makePartialCurrentLockfile: opts.makePartialCurrentLockfile,
        outdatedDependencies,
        pruneStore: opts.pruneStore,
        pruneVirtualStore: opts.pruneVirtualStore,
        publicHoistPattern: ctx.publicHoistPattern,
        registries: ctx.registries,
        rootModulesDir: ctx.rootModulesDir,
        sideEffectsCacheRead: opts.sideEffectsCacheRead,
        symlink: opts.symlink,
        skipped: ctx.skipped,
        skipRuntimes: opts.skipRuntimes,
        storeController: opts.storeController,
        virtualStoreDir: ctx.virtualStoreDir,
        virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
        wantedLockfile: newLockfile,
        wantedToBeSkippedPackageIds,
        hoistWorkspacePackages: opts.hoistWorkspacePackages,
        virtualStoreOnly: opts.virtualStoreOnly,
        supportedArchitectures: opts.supportedArchitectures,
      }
    )
    stats = result.stats
    if (shouldWritePackageMap) {
      // Omit the importer self-mapping when a project has no name (see the
      // deps-restorer write): a non-package-name key like `.` would be invalid.
      const importerNames = Object.fromEntries(
        projects.map(({ manifest, id }) => [id, manifest.name])
      )
      await writePackageMap(result.currentLockfile, {
        importerNames,
        lockfileDir: ctx.lockfileDir,
        locationByDepPath: Object.fromEntries(
          Object.values(dependenciesGraph).map((node) => [node.depPath, node.dir])
        ),
        packageMapType: opts.nodePackageMapType,
        rootModulesDir: ctx.rootModulesDir,
        virtualStoreDir: ctx.virtualStoreDir,
        virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
      })
    }
    if (opts.enablePnp) {
      const importerNames = Object.fromEntries(
        projects.map(({ manifest, id }) => [id, manifest.name ?? id])
      )
      await writePnpFile(result.currentLockfile, {
        importerNames,
        lockfileDir: ctx.lockfileDir,
        virtualStoreDir: ctx.virtualStoreDir,
        virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
        registries: ctx.registries,
      })
    }

    ctx.pendingBuilds = ctx.pendingBuilds
      .filter((relDepPath) => !result.removedDepPaths.has(relDepPath))

    if (result.newDepPaths?.length) {
      if (opts.ignoreScripts) {
        // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
        ctx.pendingBuilds = ctx.pendingBuilds
          .concat(
            result.newDepPaths.filter((depPath) => dependenciesGraph[depPath].requiresBuild)
          )
      }
      if (!opts.ignoreScripts || Object.keys(opts.patchedDependencies ?? {}).length > 0) {
        // postinstall hooks
        const depPaths = Object.keys(dependenciesGraph) as DepPath[]
        const rootNodes = depPaths.filter((depPath) => dependenciesGraph[depPath].depth === 0)

        let extraEnv: Record<string, string> | undefined = opts.scriptsOpts.extraEnv
        if (opts.enablePnp) {
          extraEnv = {
            ...extraEnv,
            ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs'), extraEnv),
          }
        }
        if (opts.nodeExperimentalPackageMap && shouldWritePackageMap) {
          extraEnv = {
            ...extraEnv,
            ...makeNodePackageMapOption(path.join(ctx.rootModulesDir, PACKAGE_MAP_FILENAME), extraEnv),
          }
        }
        // Dependency lifecycle scripts must not run on an unverified lockfile.
        await opts.verifyLockfile?.()
        ignoredBuilds = (await buildModules(dependenciesGraph, rootNodes, {
          allowBuild: opts.allowBuild,
          childConcurrency: opts.childConcurrency,
          depsStateCache,
          depsToBuild: new Set(result.newDepPaths),
          extraBinPaths: ctx.extraBinPaths,
          extraNodePaths: ctx.extraNodePaths,
          extraEnv,
          ignoreScripts: opts.ignoreScripts,
          lockfileDir: ctx.lockfileDir,
          optional: opts.include.optionalDependencies,
          preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          rootModulesDir: ctx.virtualStoreDir,
          scriptsPrependNodePath: opts.scriptsPrependNodePath,
          scriptShell: opts.scriptShell,
          shellEmulator: opts.shellEmulator,
          sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
          storeController: opts.storeController,
          unsafePerm: opts.unsafePerm,
          userAgent: opts.userAgent,
          enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
          frozenStore: opts.frozenStore,
        })).ignoredBuilds
        if (ctx.modulesFile?.ignoredBuilds?.size) {
          ignoredBuilds ??= new Set()
          for (const ignoredBuild of ctx.modulesFile.ignoredBuilds.values()) {
            if (result.currentLockfile.packages?.[ignoredBuild] && !isBuildExplicitlyDisallowed(ignoredBuild, opts.allowBuild)) {
              ignoredBuilds.add(ignoredBuild)
            }
          }
        }
      }
    }

    const binWarn = (prefix: string, message: string) => {
      logger.info({ message, prefix })
    }
    if (result.newDepPaths?.length && !opts.virtualStoreOnly) {
      const newPkgs = props<DepPath, DependenciesGraphNode>(result.newDepPaths, dependenciesGraph)
      await linkAllBins(newPkgs, dependenciesGraph, {
        extraNodePaths: ctx.extraNodePaths,
        optional: opts.include.optionalDependencies,
        warn: binWarn.bind(null, opts.lockfileDir),
      })
    }

    if (!opts.virtualStoreOnly) await Promise.all(projects.map(async (project, index) => {
      let linkedPackages!: string[]
      if (ctx.publicHoistPattern?.length && path.relative(project.rootDir, opts.lockfileDir) === '') {
        linkedPackages = await linkBins(project.modulesDir, project.binsDir, {
          allowExoticManifests: true,
          preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          projectManifest: project.manifest,
          extraNodePaths: ctx.extraNodePaths,
          warn: binWarn.bind(null, project.rootDir),
        })
      } else {
        const directPkgs = [
          ...props<DepPath, DependenciesGraphNode>(
            Array.from(dependenciesByProjectId[project.id].values()).filter((depPath) => !ctx.skipped.has(depPath)),
            dependenciesGraph
          ),
          ...linkedDependenciesByProjectId[project.id].map(({ pkgId }) => ({
            dir: path.join(project.rootDir, pkgId.substring(5)),
            fetching: undefined,
          })),
        ]
        linkedPackages = await linkBinsOfPackages(
          (
            await Promise.all(
              directPkgs.map(async (dep) => {
                const manifest = (await dep.fetching?.())?.bundledManifest ?? await safeReadProjectManifestOnly(dep.dir)
                return {
                  location: dep.dir,
                  manifest,
                }
              })
            )
          )
            .filter(({ manifest }) => manifest != null) as Array<{ location: string, manifest: DependencyManifest }>,
          project.binsDir,
          {
            extraNodePaths: ctx.extraNodePaths,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          }
        )
      }
      const projectToInstall = projects[index]
      if (opts.global && projectToInstall.mutation.includes('install')) {
        for (const pkg of projectToInstall.wantedDependencies) {
          // This warning is never printed currently during "pnpm link --global"
          // due to the following issue: https://github.com/pnpm/pnpm/issues/4761
          if (pkg.alias && !linkedPackages?.includes(pkg.alias)) {
            logger.warn({ message: `${pkg.alias} has no binaries`, prefix: opts.lockfileDir })
          }
        }
      }
    }))

    const projectsWithTargetDirs = getProjectsWithTargetDirs(projects, newLockfile, dependenciesGraph)
    const currentLockfileDir = path.join(ctx.rootModulesDir, '.pnpm')
    await Promise.all([
      opts.useLockfile && opts.saveLockfile
        ? writeLockfilesAndRecordVerified({
          currentLockfile: result.currentLockfile,
          currentLockfileDir,
          wantedLockfile: newLockfile,
          wantedLockfileDir: ctx.lockfileDir,
          cacheDir: opts.cacheDir,
          resolutionVerifiers: opts.resolutionVerifiers,
          ...lockfileOpts,
        })
        : writeCurrentLockfile(ctx.virtualStoreDir, result.currentLockfile),
      (async () => {
        if (result.currentLockfile.packages === undefined && result.removedDepPaths.size === 0) {
          return Promise.resolve()
        }
        const injectedDeps: Record<string, string[]> = {}
        for (const project of projectsWithTargetDirs) {
          if (project.targetDirs.length > 0) {
            injectedDeps[project.id] = project.targetDirs.map((targetDir) => path.relative(opts.lockfileDir, targetDir))
          }
        }
        return writeModulesManifest(ctx.rootModulesDir, {
          ...ctx.modulesFile,
          hoistedDependencies: result.newHoistedDependencies,
          hoistPattern: ctx.hoistPattern,
          included: ctx.include,
          injectedDeps,
          ignoredBuilds,
          layoutVersion: LAYOUT_VERSION,
          nodeLinker: opts.nodeLinker,
          packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
          pendingBuilds: ctx.pendingBuilds,
          publicHoistPattern: ctx.publicHoistPattern,
          virtualStoreOnly: opts.virtualStoreOnly,
          prunedAt: opts.pruneVirtualStore || ctx.modulesFile == null
            ? new Date().toUTCString()
            : ctx.modulesFile.prunedAt,
          registries: ctx.registries,
          skipped: Array.from(ctx.skipped),
          storeDir: ctx.storeDir,
          virtualStoreDir: ctx.virtualStoreDir,
          virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
          allowBuilds: opts.allowBuilds,
        })
      })(),
    ])
    if (!opts.ignoreScripts && !opts.virtualStoreOnly) {
      if (opts.enablePnp) {
        opts.scriptsOpts.extraEnv = {
          ...opts.scriptsOpts.extraEnv,
          ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs'), opts.scriptsOpts.extraEnv),
        }
      }
      if (opts.nodeExperimentalPackageMap && shouldWritePackageMap) {
        opts.scriptsOpts.extraEnv = {
          ...opts.scriptsOpts.extraEnv,
          ...makeNodePackageMapOption(path.join(ctx.rootModulesDir, PACKAGE_MAP_FILENAME), opts.scriptsOpts.extraEnv),
        }
      }
      const projectsToBeBuilt = projectsWithTargetDirs.filter(({ mutation }) => mutation === 'install') as ProjectToBeInstalled[]
      // The projects' own lifecycle scripts import dependency code linked
      // from the lockfile, so they are held to the same gate as dependency
      // builds — also when no new dep paths made the buildModules branch run.
      await opts.verifyLockfile?.()
      await runLifecycleHooksConcurrently(['preinstall', 'install', 'postinstall', 'preprepare', 'prepare', 'postprepare'],
        projectsToBeBuilt,
        opts.childConcurrency,
        opts.scriptsOpts
      )
    }
  } else {
    if (opts.useLockfile && opts.saveLockfile && !isInstallationOnlyForLockfileCheck) {
      await writeWantedLockfileAndRecordVerified({
        lockfileDir: ctx.lockfileDir,
        lockfile: newLockfile,
        cacheDir: opts.cacheDir,
        resolutionVerifiers: opts.resolutionVerifiers,
        ...lockfileOpts,
      })
    }

    if (opts.nodeLinker !== 'hoisted' && opts.runPacquet == null) {
      // This is only needed because otherwise the reporter will hang.
      // Skipped when pacquet is about to take over the materialization
      // phase: the default reporter completes the progress stream for
      // this prefix on `importing_done`, so emitting it from the
      // lockfileOnly resolve pass would prematurely close the stream
      // and pacquet's own `importing_started` / progress events would
      // render to a stale stream. Pacquet emits its own
      // `importing_done` after the install, which closes the stream
      // normally.
      stageLogger.debug({
        prefix: opts.lockfileDir,
        stage: 'importing_done',
      })
    }
  }

  await waitTillAllFetchingsFinish()
  const depsRequiringBuild: DepPath[] = []
  if (opts.returnListOfDepsRequiringBuild) {
    await Promise.all(Object.entries(dependenciesGraph).map(async ([depPath, node]) => {
      if (node?.fetching == null) return // We cannot detect if a skipped optional dependency requires build
      const { files } = await node.fetching()
      if (files.requiresBuild) {
        depsRequiringBuild.push(depPath as DepPath)
      }
    }))
  }

  reportPeerDependencyIssues(peerDependencyIssuesByProjects, {
    lockfileDir: opts.lockfileDir,
    strictPeerDependencies: opts.strictPeerDependencies,
    rules: opts.peerDependencyRules,
  })

  // Skipped when pacquet will take over the materialization. The
  // default reporter's `reportSummary` `take(1)`s the first summary
  // event and combines it with whatever `pkgsDiff` it has at that
  // moment — which is empty here, since pacquet hasn't emitted its
  // per-direct-dep `pnpm:root` events yet. Letting pnpm fire summary
  // now would lock in an empty diff. Pacquet emits its own
  // `pnpm:summary` after the install completes, by which point its
  // root events have populated the diff.
  if (!opts.omitSummaryLog && opts.runPacquet == null) {
    summaryLogger.debug({ prefix: opts.lockfileDir })
  }

  // Similar to the sequencing for when the original wanted lockfile is
  // copied, the new lockfile passed here should be as close as possible to
  // what will eventually be written to disk. Ex: peers should be resolved,
  // the afterAllResolved hook has been applied, etc.
  if (originalLockfileForCheck != null) {
    opts.lockfileCheck?.(originalLockfileForCheck, newLockfile)
  }

  return {
    updatedCatalogs,
    newLockfile,
    projects: projects.map(({ id, manifest, rootDir }) => ({
      manifest,
      peerDependencyIssues: peerDependencyIssuesByProjects[id],
      rootDir,
    })),
    stats,
    depsRequiringBuild,
    ignoredBuilds,
    resolutionPolicyViolations,
    dryRunResult: (opts.dryRun && originalLockfileForCheck != null)
      ? { originalLockfile: originalLockfileForCheck, wantedLockfile: newLockfile }
      : undefined,
  }
}

function allMutationsAreInstalls (projects: MutatedProject[]): boolean {
  return projects.every((project) => project.mutation === 'install' && !project.update && !project.updateMatching)
}

/**
 * The `InstallFunctionResult` for an install pacquet resolved and
 * materialized end-to-end. pacquet wrote `pnpm-lock.yaml` and the
 * `node_modules` tree itself. `ctx.wantedLockfile` has already been
 * refreshed from disk, and pacquet reports its own stats / ignored-builds
 * via NDJSON, so the structured `stats` / `ignoredBuilds` fall back to
 * their no-op defaults. Resolution-policy handlers are guarded out before
 * this path, so there are no command-layer policy violations to return.
 * Manifests are returned unchanged — this path only runs for plain
 * installs, which don't rewrite `package.json`.
 */
function pacquetResolveResult (projects: ImporterToUpdate[], ctx: PnpmContext): InstallFunctionResult {
  return {
    newLockfile: ctx.wantedLockfile,
    projects: projects.map((project) => ({
      manifest: project.originalManifest ?? project.manifest,
      rootDir: project.rootDir,
    })),
    depsRequiringBuild: [],
    resolutionPolicyViolations: [],
  }
}

/**
 * Run the pacquet binary if it's configured, otherwise run the JS
 * `headlessInstall`. Callers can hand off any code path that materializes
 * an already-resolved lockfile (workspace partial install, hoisted
 * linker, pnpr server install, frozen install) without restating the
 * delegation choice.
 *
 * Pacquet reads the wanted lockfile from disk and produces its own
 * `pnpm:stats` / `pnpm:ignored-scripts` log events that drive the
 * reporter. The structured stats / ignoredBuilds return values that
 * `headlessInstall` produces aren't recovered here — pacquet doesn't
 * surface them through any return path — so callers get `undefined` for
 * both. `mutateModules` already tolerates that (it falls back to a zero
 * stats record and a no-op ignoredBuilds iteration).
 */
async function materializeOrDelegate (
  opts: {
    mergeGitBranchLockfiles?: boolean
    runPacquet?: { run: (opts?: { filterResolvedProgress?: boolean }) => Promise<void> }
    saveLockfile?: boolean
    useGitBranchLockfile?: boolean
    useLockfile?: boolean
  },
  runHeadlessInstall: () => Promise<{ stats: InstallationResultStats, ignoredBuilds: IgnoredBuilds | undefined }>
): Promise<{ stats?: InstallationResultStats, ignoredBuilds?: IgnoredBuilds }> {
  if (
    opts.runPacquet != null &&
    opts.useLockfile !== false &&
    opts.saveLockfile !== false &&
    opts.useGitBranchLockfile !== true &&
    opts.mergeGitBranchLockfiles !== true
  ) {
    // Reached only from the resolve-then-materialize call sites
    // (workspace-partial, hoisted-linker, pnpr server install). Each ran a
    // lockfileOnly resolve pass that emitted one
    // `pnpm:progress status:resolved` per package, so pacquet's
    // duplicate `resolved` events would double the reporter's count.
    await opts.runPacquet.run({ filterResolvedProgress: true })
    return {}
  }
  return runHeadlessInstall()
}

const installInContext: InstallFunction = async (projects, ctx, opts) => {
  try {
    const isPathInsideWorkspace = isSubdir.bind(null, opts.lockfileDir)
    if (!opts.frozenLockfile && opts.useLockfile) {
      const allProjectsLocatedInsideWorkspace = Object.values(ctx.projects)
        .filter((project) => isPathInsideWorkspace(project.rootDirRealPath ?? project.rootDir))
      if (allProjectsLocatedInsideWorkspace.length > projects.length && !isCheckOnlyInstall(opts) && opts.enableModulesDir) {
        const newProjects = [...projects]
        const getWantedDepsOpts = {
          autoInstallPeers: opts.autoInstallPeers,
          includeDirect: opts.includeDirect,
          updateWorkspaceDependencies: false,
          injectWorkspacePackages: opts.injectWorkspacePackages,
        }
        const _isWantedDepBareSpecifierSame = isWantedDepBareSpecifierSame.bind(null, ctx.wantedLockfile.catalogs, opts.catalogs)
        for (const project of allProjectsLocatedInsideWorkspace) {
          if (!newProjects.some(({ rootDir }) => rootDir === project.rootDir)) {
            // This code block mirrors the installCase() function in
            // mutateModules(). Consider a refactor that combines this logic to
            // deduplicate code.
            const wantedDependencies = getWantedDependencies(project.manifest, getWantedDepsOpts)
              .map((wantedDependency) => ({ ...wantedDependency, updateSpec: true, preserveNonSemverVersionSpec: true }))
            forgetResolutionsOfPrevWantedDeps(ctx.wantedLockfile.importers[project.id], wantedDependencies, _isWantedDepBareSpecifierSame)
            newProjects.push({
              mutation: 'install',
              ...project,
              wantedDependencies,
              pruneDirectDependencies: false,
              updatePackageManifest: false,
            })
          }
        }
        const result = await installInContext(newProjects, ctx, {
          ...opts,
          lockfileOnly: true,
        })
        const { stats, ignoredBuilds } = await materializeOrDelegate(opts, () => headlessInstall({
          ...ctx,
          ...opts,
          currentEngine: {
            nodeVersion: opts.nodeVersion,
            pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
          },
          currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
          selectedProjectDirs: projects.map((project) => project.rootDir),
          allProjects: ctx.projects,
          prunedAt: ctx.modulesFile?.prunedAt,
          wantedLockfile: result.newLockfile,
          useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
          hoistWorkspacePackages: opts.hoistWorkspacePackages,
        }))
        return {
          ...result,
          stats,
          ignoredBuilds,
        }
      }
    }
    if (opts.nodeLinker === 'hoisted' && !opts.lockfileOnly && !isCheckOnlyInstall(opts) && opts.enableModulesDir) {
      const result = await _installInContext(projects, ctx, {
        ...opts,
        lockfileOnly: true,
      })
      const { stats, ignoredBuilds } = await materializeOrDelegate(opts, () => headlessInstall({
        ...ctx,
        ...opts,
        currentEngine: {
          nodeVersion: opts.nodeVersion,
          pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
        },
        currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
        selectedProjectDirs: projects.map((project) => project.rootDir),
        allProjects: ctx.projects,
        prunedAt: ctx.modulesFile?.prunedAt,
        wantedLockfile: result.newLockfile,
        useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
        hoistWorkspacePackages: opts.hoistWorkspacePackages,
      }))
      return {
        ...result,
        stats,
        ignoredBuilds,
      }
    }
    // Isolated `nodeLinker` (the default) with a non-frozen install.
    // The frozen branch is handled earlier in `tryFrozenInstall`; the
    // hoisted branch above runs a resolve-then-materialize sequence.
    if (opts.runPacquet != null && opts.useLockfile && opts.saveLockfile && !opts.useGitBranchLockfile && !opts.mergeGitBranchLockfiles && !opts.lockfileOnly && !isCheckOnlyInstall(opts) && opts.enableModulesDir) {
      // pacquet >= 0.11.7 resolves itself: hand it the whole install
      // (resolve + fetch + import + link + build, writing the lockfile)
      // in a single non-frozen pass. Only for plain installs — `add` /
      // `update` / `remove` need pnpm to mutate the manifests and
      // resolve the new specs first (pacquet's `install` reads
      // package.json from disk, which pnpm hasn't rewritten yet).
      if (opts.runPacquet.supportsResolution && !opts.frozenLockfile && opts.handleResolutionPolicyViolations == null && allMutationsAreInstalls(projects)) {
        // `configDependencies` are recorded in a YAML document prepended
        // to `pnpm-lock.yaml` — purely a pnpm concept that pacquet doesn't
        // model. Capture it before pacquet rewrites the lockfile and
        // restore it afterwards (`writeEnvLockfile` re-reads pacquet's main
        // document and re-prepends the env document), otherwise the next
        // `--frozen-lockfile` install fails its config-deps freshness gate.
        // The restore runs even if pacquet fails partway: a non-zero exit can
        // still leave a rewritten lockfile behind, so the env document must be
        // put back regardless.
        const envLockfile = await readEnvLockfile(ctx.lockfileDir)
        let pacquetError: unknown
        try {
          await opts.runPacquet.run({ resolve: true })
        } catch (err: unknown) {
          pacquetError = err
          throw err
        } finally {
          if (envLockfile != null) {
            await writeEnvLockfile(ctx.lockfileDir, envLockfile).catch((restoreErr: Error) => {
              if (pacquetError == null) {
                throw restoreErr
              }
              logger.warn({
                error: restoreErr,
                message: `Failed to restore the configDependencies document in pnpm-lock.yaml: ${restoreErr.message}`,
                prefix: ctx.lockfileDir,
              })
            })
          }
        }
        const wantedLockfile = await readWantedLockfile(ctx.lockfileDir, {
          ignoreIncompatible: opts.force || opts.ci === true,
          mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
          useGitBranchLockfile: opts.useGitBranchLockfile,
          wantedVersions: [LOCKFILE_VERSION],
        })
        if (wantedLockfile == null) {
          throw new PnpmError('PACQUET_LOCKFILE_READ_FAILED', `pacquet did not write a readable ${WANTED_LOCKFILE}`)
        }
        ctx.wantedLockfile = wantedLockfile
        return pacquetResolveResult(projects, ctx)
      }
      // Older pacquet can only materialize: split the install in two —
      // ask `_installInContext` for a `lockfileOnly` resolve pass (writes
      // `pnpm-lock.yaml`), then hand the freshly-written lockfile to
      // pacquet for the fetch / import / link / build phases. The resolve
      // pass emitted a `pnpm:progress status:resolved` per package; ask
      // pacquet to drop its own duplicates.
      const result = await _installInContext(projects, ctx, { ...opts, lockfileOnly: true })
      await opts.runPacquet.run({ filterResolvedProgress: true })
      return result
    }
    return await _installInContext(projects, ctx, opts)
  } catch (error: any) { // eslint-disable-line
    if (
      !BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code) ||
      (!ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile) ||
      !opts.updateChecksums
    ) throw error
    opts.needsFullResolution = true
    logger.warn({
      error,
      message: error.message,
      prefix: ctx.lockfileDir,
    })
    logger.error(new PnpmError(error.code, 'Refreshing the locked integrity from the registry as requested by --update-checksums. A full installation will be performed.'))
    return _installInContext(projects, ctx, opts)
  } finally {
    await opts.storeController.close()
  }
}

const limitLinking = pLimit(16)

async function linkAllBins (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    extraNodePaths?: string[]
    preferSymlinkedExecutables?: boolean
    optional: boolean
    warn: (message: string) => void
  }
): Promise<void> {
  await Promise.all(
    depNodes.map(async depNode => limitLinking(async () => linkBinsOfDependencies(depNode, depGraph, opts)))
  )
}

export class IgnoredBuildsError extends PnpmError {
  constructor (ignoredBuilds: IgnoredBuilds) {
    const packageNames = dedupePackageNamesFromIgnoredBuilds(ignoredBuilds)
    super('IGNORED_BUILDS', `Ignored build scripts: ${packageNames.join(', ')}`, {
      hint: 'Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.',
    })
  }
}

function dedupePackageNamesFromIgnoredBuilds (ignoredBuilds: IgnoredBuilds): string[] {
  return Array.from(new Set(Array.from(ignoredBuilds ?? []).map(depPath => dp.getPkgIdWithPatchHash(depPath)))).sort(lexCompare)
}

/**
 * Build injectionTargetsByDepPath from the dependenciesGraph for injected workspace packages
 * and extend projects with their target directories.
 * The dependenciesGraph already has the correct `dir` values after `extendGraph` is applied
 * (which uses the correct hash-based paths when global virtual store is enabled).
 */
function getProjectsWithTargetDirs<T extends { id: ProjectId }> (
  projects: T[],
  lockfile: LockfileObject,
  dependenciesGraph: DependenciesGraph
): Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }> {
  const injectionTargetsByDepPath = new Map<string, string[]>()
  if (lockfile.packages) {
    for (const [depPath, { resolution }] of Object.entries(lockfile.packages)) {
      if (resolution?.type === 'directory') {
        const graphNode = dependenciesGraph[depPath as DepPath]
        if (graphNode?.dir) {
          injectionTargetsByDepPath.set(depPath, [graphNode.dir])
        }
      }
    }
  }
  return extendProjectsWithTargetDirs(projects, injectionTargetsByDepPath)
}

/**
 * Whether the pnpr server path can handle this batch of mutations. The pnpr server flow
 * supports installing the manifest as-is (`install`), adding new deps
 * (`installSome`), and removing deps (`uninstallSome`). It cannot model the
 * client-side update-flag behavior (`update`/`updateMatching`/`updateToLatest`)
 * yet, so those still go through the normal client-side resolver.
 */
function canUsePnprForMutations (projects: MutatedProject[]): boolean {
  if (projects.length === 0) return false
  return projects.every((p) => {
    if (p.mutation === 'uninstallSome') return true
    if (p.mutation !== 'install' && p.mutation !== 'installSome') return false
    const m = p as InstallDepsMutation | InstallSomeDepsMutation
    return !m.update && !m.updateToLatest && m.updateMatching == null
  })
}

interface PnprNewDep {
  alias: string
  /**
   * Whether the user specified a spec (e.g. `pnpm add foo@^2`). If true, the
   * manifest already has the right value and we must preserve it. If false
   * we merged in `'latest'` and need to compute a save-prefix spec from the
   * resolved version in the lockfile after the pnpr server runs.
   */
  userSpecified: boolean
}

interface PnprInstallProject {
  rootDir: ProjectRootDir
  /** The (possibly pre-processed) manifest we send to the pnpr server. */
  manifest: ProjectManifest
  mutation: MutatedProject['mutation']
  /** Newly added deps from an `installSome` mutation. Empty otherwise. */
  newDeps: PnprNewDep[]
  /** Save-prefix config for `installSome`; applied to deps whose spec defaulted to `'latest'`. */
  pinnedVersion?: PinnedVersion
}

/**
 * Pre-process projects for the pnpr server flow:
 * - `install`: send the manifest as-is.
 * - `uninstallSome`: drop the named deps from the manifest before sending,
 *   so the pnpr server's resolution naturally produces a lockfile without them.
 * - `installSome`: parse selectors and merge them into the manifest. The
 *   pnpr server then resolves the merged manifest, and we read the resolved
 *   specifiers (with the right save-prefix applied server-side) back from
 *   the lockfile importer entries to update the client-side manifest.
 *
 * Returns null if the projects don't map cleanly to allProjects (caller
 * should fall through to the normal flow).
 */
async function preparePnprProjects (
  projects: MutatedProject[],
  opts: MutateModulesOptions
): Promise<PnprInstallProject[] | null> {
  const allProjects = opts.allProjects ?? []
  const mutationByRootDir = new Map<ProjectRootDir, MutatedProject>()
  for (const p of projects) {
    mutationByRootDir.set(p.rootDir, p)
  }
  // Include every workspace project, not just the mutated ones — otherwise
  // the pnpr server's resulting lockfile would only contain the targeted importer
  // and `headlessInstall` (or a later install) would crash on the missing
  // entries for the other workspace projects. Projects without a mutation
  // are sent with their current manifest (no-op for resolution).
  const targetSet: Array<{ rootDir: ProjectRootDir, manifest: ProjectManifest, mutation?: MutatedProject }> =
    allProjects.length > 0
      ? allProjects.map((ap) => ({
        rootDir: ap.rootDir,
        manifest: ap.manifest,
        mutation: mutationByRootDir.get(ap.rootDir),
      }))
      : projects.map((p) => {
        const proj = allProjects.find((ap) => ap.rootDir === p.rootDir)
        return {
          rootDir: p.rootDir,
          manifest: proj?.manifest ?? ({} as ProjectManifest),
          mutation: p,
        }
      })
  // Bail to the normal flow if any mutated project isn't in allProjects —
  // we can't pre-process its manifest correctly.
  for (const p of projects) {
    if (!targetSet.some((t) => t.rootDir === p.rootDir)) return null
  }
  return Promise.all(targetSet.map(async (t) => {
    let manifest: ProjectManifest = clone(t.manifest)
    const newDeps: PnprNewDep[] = []
    const mutation = t.mutation
    let pinnedVersion: PinnedVersion | undefined
    if (mutation?.mutation === 'uninstallSome') {
      manifest = await removeDeps(manifest, mutation.dependencyNames, {
        prefix: mutation.rootDir,
        saveType: mutation.targetDependenciesField,
      })
    } else if (mutation?.mutation === 'installSome') {
      manifest = mergeInstallSelectors(manifest, mutation)
      pinnedVersion = mutation.pinnedVersion
      for (const sel of mutation.dependencySelectors) {
        const parsed = parseWantedDependency(sel)
        if (parsed.alias) {
          newDeps.push({ alias: parsed.alias, userSpecified: parsed.bareSpecifier != null })
        }
      }
    }
    return {
      rootDir: t.rootDir,
      manifest,
      mutation: mutation?.mutation ?? 'install',
      newDeps,
      pinnedVersion,
    }
  }))
}

/**
 * Merge `installSome` selectors into the manifest, choosing the target
 * dependency field per the mutation's `targetDependenciesField` (or the
 * existing field if the dep is already in the manifest, defaulting to
 * `dependencies`). Selectors without a version use `'latest'` so the
 * pnpr server's resolver picks the newest matching release.
 */
function mergeInstallSelectors (manifest: ProjectManifest, mutation: InstallSomeDepsMutation): ProjectManifest {
  const target = mutation.targetDependenciesField
  const fieldsToClear: DependenciesField[] = ['dependencies', 'devDependencies', 'optionalDependencies']
  for (const sel of mutation.dependencySelectors) {
    const parsed = parseWantedDependency(sel)
    if (!parsed.alias) continue
    const alias = parsed.alias
    const field: DependenciesField = target ?? guessDepField(alias, manifest) ?? 'dependencies'
    const spec = parsed.bareSpecifier ?? findExistingSpec(alias, manifest) ?? 'latest'
    manifest[field] = manifest[field] ?? {}
    manifest[field]![alias] = spec
    // If `targetDependenciesField` is set, also remove the alias from the
    // other fields — matches the normal flow's behavior.
    if (target) {
      for (const other of fieldsToClear) {
        if (other !== target) delete manifest[other]?.[alias]
      }
    }
    if (mutation.peer) {
      manifest.peerDependencies = manifest.peerDependencies ?? {}
      manifest.peerDependencies[alias] = manifest.peerDependencies[alias] ?? spec
    }
  }
  return manifest
}

function guessDepField (alias: string, manifest: ProjectManifest): DependenciesField | undefined {
  if (manifest.dependencies?.[alias] != null) return 'dependencies'
  if (manifest.devDependencies?.[alias] != null) return 'devDependencies'
  if (manifest.optionalDependencies?.[alias] != null) return 'optionalDependencies'
  return undefined
}

function findExistingSpec (alias: string, manifest: ProjectManifest): string | undefined {
  return manifest.dependencies?.[alias] ??
    manifest.devDependencies?.[alias] ??
    manifest.optionalDependencies?.[alias]
}

/**
 * After the pnpr server resolves, copy the lockfile importer's per-dep specifier
 * (which the server's resolver computed with the right save-prefix) back
 * into the client manifest for any newly added aliases. We rely on the
 * lockfile because the pnpr server applies catalog substitution,
 * normalizedBareSpecifier, and save-prefix logic during resolution.
 */
function applyResolvedSpecsFromLockfile (
  manifest: ProjectManifest,
  importerSnapshot: ProjectSnapshot | undefined,
  newDeps: PnprNewDep[],
  pinnedVersion?: PinnedVersion
): ProjectManifest {
  if (!importerSnapshot || newDeps.length === 0) return manifest
  // In-memory ProjectSnapshot stores resolved versions in `dependencies`
  // (alias → resolved version) and original specs in `specifiers` (alias →
  // user spec). The on-disk YAML shape pairs them per entry — the reader
  // splits them. Read both and compute the save-prefix spec client-side.
  for (const dep of newDeps) {
    // User explicitly specified a spec (e.g. `foo@^2`) — the merged manifest
    // already has the right value, don't touch it.
    if (dep.userSpecified) continue
    for (const field of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
      const resolvedVersion = importerSnapshot[field]?.[dep.alias]
      if (!resolvedVersion || manifest[field]?.[dep.alias] == null) continue
      // The pnpr server resolved the tree but, on the plain-install path, it
      // writes the user's raw spec (`'latest'`) into the lockfile specifier
      // rather than normalizing to a save-prefix range. Compute the
      // save-prefix spec client-side from the resolved version.
      const savePrefixSpec = createVersionSpecFromResolvedVersion(resolvedVersion, pinnedVersion)
      manifest[field]![dep.alias] = savePrefixSpec ?? resolvedVersion
    }
  }
  return manifest
}

/**
 * Drives the pnpr server path for a `mutateModules` call across one or more
 * projects. Returns null if the call can't be served by the pnpr server (e.g. one
 * of the projects isn't in `allProjects`).
 */
async function mutateModulesViaPnpr (
  projects: MutatedProject[],
  opts: MutateModulesOptions
): Promise<MutateModulesResult | null> {
  const pnprProjects = await preparePnprProjects(projects, opts)
  if (!pnprProjects) return null

  // installViaPnprServer runs the headless install for the first
  // project's root and the workspace path for the rest. Pass the
  // pre-processed manifests so resolution sees the post-mutation state.
  const result = await installViaPnprServer(
    pnprProjects[0].manifest,
    pnprProjects[0].rootDir,
    opts,
    pnprProjects.map((p) => ({ rootDir: p.rootDir, manifest: p.manifest }))
  )

  // For installSome projects, copy resolved specs from the lockfile importer
  // entries back into the client manifest so save-prefix/catalog/etc. take
  // effect (the server applies these during its resolution step).
  const lockfileDir = opts.lockfileDir ?? projects[0].rootDir
  const mutatedRootDirs = new Set(projects.map((p) => p.rootDir))
  const updatedProjects = pnprProjects
    .filter((p) => mutatedRootDirs.has(p.rootDir))
    .map((p) => {
      if (p.mutation === 'installSome' && p.newDeps.length > 0) {
        // Lockfile importer keys are POSIX-normalized paths.
        const relative = path.relative(lockfileDir, p.rootDir).split(path.sep).join('/')
        const importerId = (relative || '.') as ProjectId
        const snapshot = result.lockfile?.importers?.[importerId]
        p.manifest = applyResolvedSpecsFromLockfile(p.manifest, snapshot, p.newDeps, p.pinnedVersion)
      }
      return { rootDir: p.rootDir, manifest: p.manifest }
    })

  return {
    updatedProjects,
    stats: result.stats,
    ignoredBuilds: result.ignoredBuilds,
  } as MutateModulesResult
}

/**
 * Walk the resolved lockfile to determine which override selectors matched
 * at least one dependency. Used on the pnpr-server path where the resolver
 * runs server-side and does not report applied selectors back.
 *
 * A selector is considered matched if its target name appears as a
 * dependency key in any importer or package snapshot. The target's
 * version range (bareSpecifier) is NOT checked against the resolved
 * version because the lockfile stores post-override values — the
 * override already changed the version, so comparing the new version
 * against the old selector range would produce false positives (e.g.
 * `foo@^1: 2.0.0` resolves to 2.0.0, which doesn't satisfy ^1).
 * Parent-scoped selectors check both resolved packages and workspace
 * project manifests (importers) for parent identity.
 *
 * `projectManifests` maps importer IDs to the workspace project's
 * manifest, so parent-scoped overrides can match project names that
 * don't appear in `lockfile.packages`.
 */
export function findAppliedOverrideSelectorsFromLockfile (
  lockfile: LockfileObject,
  parsedOverrides: Array<{ selector: string, parentPkg?: { name: string, bareSpecifier?: string }, targetPkg: { name: string, bareSpecifier?: string } }>,
  projectManifests: Array<{ importerId: string, manifest: ProjectManifest }> = []
): Set<string> {
  const applied = new Set<string>()

  for (const override of parsedOverrides) {
    const targetName = override.targetPkg.name

    if (override.parentPkg != null) {
      const parentName = override.parentPkg.name
      const parentRange = override.parentPkg.bareSpecifier
      const parentRangeValid = parentRange == null || semver.validRange(parentRange) != null

      // Check workspace project manifests as potential parent matches.
      // Importer snapshots don't carry name/version, so we match against
      // the manifest and then look up the importer's dependencies.
      for (const { importerId, manifest: projectManifest } of projectManifests) {
        if (projectManifest.name !== parentName) continue
        if (parentRange != null) {
          const projectVersion = projectManifest.version
          if (projectVersion == null) continue
          if (!parentRangeValid || !semver.satisfies(projectVersion, parentRange)) continue
        }
        const importer = lockfile.importers[importerId as ProjectId]
        if (importer == null) continue
        if (
          depEntryMatches(importer.dependencies, targetName) ||
          depEntryMatches(importer.devDependencies, targetName) ||
          depEntryMatches(importer.optionalDependencies, targetName)
        ) {
          applied.add(override.selector)
          break
        }
      }
      if (applied.has(override.selector)) continue

      // Check resolved packages as potential parent matches.
      for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
        const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
        if (name !== parentName) continue
        if (parentRange != null && (version == null || !parentRangeValid || !semver.satisfies(version, parentRange))) continue
        if (
          depEntryMatches(snapshot.dependencies, targetName) ||
          depEntryMatches(snapshot.optionalDependencies, targetName) ||
          depEntryMatches(snapshot.peerDependencies, targetName)
        ) {
          applied.add(override.selector)
          break
        }
      }
    } else {
      for (const importer of Object.values(lockfile.importers)) {
        if (
          depEntryMatches(importer.dependencies, targetName) ||
          depEntryMatches(importer.devDependencies, targetName) ||
          depEntryMatches(importer.optionalDependencies, targetName)
        ) {
          applied.add(override.selector)
          break
        }
      }
      if (applied.has(override.selector)) continue
      for (const snapshot of Object.values(lockfile.packages ?? {})) {
        if (
          depEntryMatches(snapshot.dependencies, targetName) ||
          depEntryMatches(snapshot.optionalDependencies, targetName) ||
          depEntryMatches(snapshot.peerDependencies, targetName)
        ) {
          applied.add(override.selector)
          break
        }
      }
    }
  }

  return applied
}

/**
 * Check whether a resolved-dependency map contains `targetName`. The
 * lockfile stores post-override resolved versions; the target's version
 * range is intentionally NOT checked here because the override already
 * changed the version (see findAppliedOverrideSelectorsFromLockfile).
 */
function depEntryMatches (
  deps: Record<string, string> | undefined,
  targetName: string
): boolean {
  if (deps == null) return false
  return deps[targetName] != null
}

/**
 * When a pnpr server is configured, resolve dependencies server-side,
 * then run a headless install that fetches tarballs from the registries
 * and links packages into node_modules — like a normal install.
 */
async function installViaPnprServer (
  manifest: ProjectManifest,
  rootDir: ProjectRootDir,
  opts: Opts,
  allInstallProjects?: Array<{ rootDir: ProjectRootDir, manifest: ProjectManifest }>
): Promise<InstallResult & { stats: InstallationResultStats, lockfile: LockfileObject }> {
  // The pnpr server path re-resolves and persists new `index.db` entries plus a
  // freshly written lockfile, so it inherently writes the store. `frozenStore`
  // promises the store is complete and read-only, so the two are mutually
  // exclusive — and the unconditional pnpr gate means this path runs even under
  // `--offline --frozen-lockfile`, so refuse up front with guidance instead of
  // crashing later on the read-only `index.db` open.
  if (opts.frozenStore) {
    throw new PnpmError(
      'FROZEN_STORE_INCOMPATIBLE_WITH_PNPR',
      'The pnpr server resolves dependencies and writes new entries into the store, which is opened read-only when frozenStore is enabled.',
      { hint: 'Disable the pnpr server (unset `--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) so the install reads from the existing store, or unset `frozenStore` to allow store writes.' }
    )
  }
  // The pnpr server path skips client-side resolution, so resolver-side policies
  // can't be enforced locally. `minimumReleaseAge` is forwarded to the
  // pnpr server and enforced server-side. `trustPolicy` has no server-side
  // counterpart yet, so refuse to run under it instead of silently
  // letting through a lockfile the local verifier would reject.
  if (opts.trustPolicy === 'no-downgrade') {
    throw new PnpmError(
      'TRUST_POLICY_INCOMPATIBLE_WITH_PNPR',
      'The pnpr server does not yet enforce `trustPolicy: no-downgrade`, so running an install through it under this policy would produce a lockfile that the local verifier rejects.',
      { hint: 'Unset `trustPolicy` for this install, or disable the pnpr server (unset `--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) so resolution runs locally and the trust check applies.' }
    )
  }
  const { resolveViaPnprServer } = await import('@pnpm/pnpr.client')
  const { createGetAuthHeaderByURI } = await import('@pnpm/network.auth-header')

  // Identify the caller to pnpr's gate. The client does not forward its
  // upstream registry credentials: pnpr selects upstream credentials from
  // its own route policy, so they never travel in the request body.
  const configByUri = opts.configByUri ?? {}
  const pnprAuthorization = createGetAuthHeaderByURI(configByUri)(opts.pnprServer!)

  try {
    const lockfileDir = opts.lockfileDir ?? rootDir

    // Read the existing lockfile (if any) in its on-disk shape — that's
    // what the pnpr server protocol carries, so no conversion is needed before
    // sending it.
    const existingLockfile = await readWantedLockfileFile(lockfileDir, {
      ignoreIncompatible: true,
    }).catch(() => null)

    logger.info({ message: 'Resolving dependencies via the pnpr server', prefix: rootDir })

    // Build projects list for workspace support.
    // Normalize separators to POSIX — on Windows `path.relative` returns
    // backslashes, which the pnpr server rejects (it treats `\` as an
    // unsafe/YAML-injection character and normalizes paths as POSIX).
    const projectsList = allInstallProjects && allInstallProjects.length > 1
      ? allInstallProjects.map(p => ({
        dir: (path.relative(lockfileDir, p.rootDir) || '.').split(path.sep).join('/'),
        dependencies: p.manifest.dependencies,
        devDependencies: p.manifest.devDependencies,
        optionalDependencies: p.manifest.optionalDependencies,
      }))
      : undefined

    const { lockfile, stats: pnprStats } = await resolveViaPnprServer({
      registryUrl: opts.pnprServer!,
      dependencies: projectsList ? undefined : manifest.dependencies,
      devDependencies: projectsList ? undefined : manifest.devDependencies,
      optionalDependencies: projectsList ? undefined : manifest.optionalDependencies,
      projects: projectsList,
      registry: opts.registries?.default,
      namedRegistries: opts.namedRegistries,
      authorization: pnprAuthorization,
      overrides: opts.overrides,
      minimumReleaseAge: opts.minimumReleaseAge,
      lockfile: existingLockfile ?? undefined,
    })

    await writeWantedLockfileAndRecordVerified({
      lockfileDir,
      lockfile,
      cacheDir: opts.cacheDir,
      resolutionVerifiers: opts.resolutionVerifiers,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })

    logger.info({
      message: `Resolved ${pnprStats.totalPackages} packages`,
      prefix: rootDir,
    })

    // The pnpr protocol resolves with overrides but does not report
    // which selectors matched. Scan the resolved lockfile to determine
    // that, then emit unused-override events for unmatched selectors —
    // same behavior as the local-resolver path. Emitted before
    // 'resolution_done' so the reporter's buffer captures them.
    const parsedOverrides = parseOverrides(opts.overrides ?? {}, opts.catalogs ?? {})
    if (parsedOverrides.length > 0) {
      // Build the project-manifest list so parent-scoped overrides can
      // match workspace project names (which appear in lockfile.importers,
      // not lockfile.packages). Importer IDs are POSIX-normalized relative
      // paths from the lockfileDir — same computation as `projectsList`.
      const projectManifests = (allInstallProjects ?? [{ rootDir, manifest }]).map(p => ({
        importerId: (path.relative(lockfileDir, p.rootDir) || '.').split(path.sep).join('/'),
        manifest: p.manifest,
      }))
      const applied = findAppliedOverrideSelectorsFromLockfile(lockfile, parsedOverrides, projectManifests)
      for (const override of parsedOverrides) {
        if (!applied.has(override.selector)) {
          unusedOverrideLogger.debug({
            prefix: lockfileDir,
            selector: override.selector,
          })
        }
      }
    }
    stageLogger.debug({
      prefix: lockfileDir,
      stage: 'resolution_done',
    })

    // `--lockfile-only`: the pnpr server resolved and we wrote the lockfile, but
    // pnpm fetches nothing and links nothing in this mode — stop before the
    // headless install. See https://github.com/pnpm/pnpm/issues/12146.
    if (opts.lockfileOnly) {
      return {
        updatedCatalogs: undefined,
        updatedManifest: manifest,
        ignoredBuilds: undefined,
        stats: { added: 0, removed: 0, linkedToRoot: 0 },
        lockfile,
        resolutionPolicyViolations: [],
      }
    }

    // The pnpr server only resolves; it serves no file content. Fetch every
    // tarball from the registries with the regular store controller, in
    // parallel, exactly like a normal install. See
    // https://github.com/pnpm/pnpm/issues/12230.
    const headlessOpts = {
      ...opts,
      dir: rootDir as string,
      lockfileDir,
      engineStrict: opts.engineStrict ?? false,
      ignoreScripts: opts.ignoreScripts ?? false,
      sideEffectsCacheRead: opts.sideEffectsCacheRead ?? false,
      sideEffectsCacheWrite: opts.sideEffectsCacheWrite ?? false,
      symlink: opts.symlink ?? true,
      enableModulesDir: opts.enableModulesDir ?? true,
      include: opts.include ?? { dependencies: true, devDependencies: true, optionalDependencies: true },
      currentEngine: {
        nodeVersion: opts.nodeVersion,
        pnpmVersion: opts.packageManager?.version ?? '',
      },
      selectedProjectDirs: (allInstallProjects ?? [{ rootDir }]).map(p => p.rootDir),
      allProjects: Object.fromEntries(
        (allInstallProjects ?? [{ rootDir, manifest }]).map((p, i) => [
          p.rootDir,
          {
            binsDir: path.join(p.rootDir, 'node_modules', '.bin'),
            buildIndex: i,
            id: (path.relative(lockfileDir, p.rootDir) || '.') as ProjectId,
            manifest: p.manifest,
            modulesDir: path.join(p.rootDir, 'node_modules'),
            rootDir: p.rootDir,
          },
        ])
      ),
      hoistedDependencies: {},
      pendingBuilds: [] as string[],
      skipped: new Set<DepPath>(),
      wantedLockfile: lockfile,
    }
    const { ignoredBuilds, stats } = await materializeOrDelegate(
      opts,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      () => headlessInstall(headlessOpts as any)
    )

    return {
      updatedCatalogs: undefined,
      updatedManifest: manifest,
      ignoredBuilds,
      // Pacquet doesn't surface a structured stats return; default to
      // zeros so the pnpr server's non-optional `stats` slot is filled.
      // The reporter still renders accurate counts from pacquet's
      // `pnpm:stats` log events.
      stats: stats ?? { added: 0, removed: 0, linkedToRoot: 0 },
      lockfile,
      // Server-side resolution (pnpr server) enforces `minimumReleaseAge`
      // itself — the pnpr server picks only mature versions and the lockfile
      // can't contain immature entries to auto-collect. `trustPolicy` is
      // guarded above (we refuse to enter this path when it's set), so
      // there's nothing for the install command to react to here.
      resolutionPolicyViolations: [],
    }
  } finally {
    // Close the storeController to flush queued StoreIndex writes — the
    // normal install path does the same; skipping it here would leave
    // pending writes on disk and diverge from lifecycle expectations.
    await opts.storeController.close()
  }
}
