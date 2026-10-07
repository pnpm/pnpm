import path from 'node:path'

import { createAllowBuildFunction } from '@pnpm/building.policy'
import { ignoredScriptsLogger } from '@pnpm/core-loggers'
import {
  makeProjectNodePathOption,
  runLifecycleHook,
  type RunLifecycleHooksConcurrentlyOptions,
} from '@pnpm/exec.lifecycle'
import { getContext, type PnpmContext } from '@pnpm/installing.context'
import { cleanGitBranchLockfiles, getWantedLockfileName } from '@pnpm/lockfile.fs'
import { streamParser } from '@pnpm/logger'
import type { IncludedDependencies, ProjectManifest } from '@pnpm/types'
import { verifiedFileIntegritySince, verifiedFileIntegritySnapshot } from '@pnpm/worker'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'

import { checkCustomResolverForceResolve } from './checkCustomResolverForceResolve.js'
import { extendOptions, type ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { dedupePackageNamesFromIgnoredBuilds, settleIgnoredBuilds } from './ignoredBuilds.js'
import {
  allMutationsAreInstalls,
  cacheExpired,
  DEV_PREINSTALL,
  installRunsDevPreinstall,
  isCheckOnlyInstall,
  matchUpdateTargetsReplacedEverywhere,
  rootProjectRunsPreinstallEarly,
} from './installPredicates.js'
import type {
  InnerInstallResult,
  MutatedProject,
  MutateModulesOptions,
  MutateModulesResult,
  MutationRun,
  MutationRunBase,
} from './mutationTypes.js'
import { canUsePnprForMutations, mutateModulesViaPnpr, pnprCanRunPnpmfile } from './pnpr.js'
import { reportVerifiedFileIntegrity } from './reportVerifiedFileIntegrity.js'
import { runMutations } from './runMutations.js'
import { validateModules } from './validateModules.js'
import { verifyLockfileResolutions } from './verifyLockfileResolutions.js'

export async function mutateModules (
  projects: MutatedProject[],
  maybeOpts: MutateModulesOptions
): Promise<MutateModulesResult> {
  const detachReporter = attachReporter(maybeOpts?.reporter)

  const opts = extendOptions(maybeOpts)

  // Taken before any fetching so the store-verification figures this
  // install reports are its own. See the tally in `@pnpm/worker` for
  // the one case this diff cannot separate.
  const verifiedFileIntegrityBaseline = verifiedFileIntegritySnapshot()

  // When a pnpr server is configured, use server-side resolution.
  if (opts.pnprServer && canUsePnprForMutations(projects, opts) && pnprCanRunPnpmfile(opts)) {
    const pnprResult = await mutateModulesViaPnpr(projects, opts)
    if (pnprResult) {
      // This path materializes packages of its own, so it verifies the
      // store like any other install and returns without reaching the
      // report below.
      reportVerifiedFileIntegrity(verifiedFileIntegritySince(verifiedFileIntegrityBaseline))
      detachReporter()
      return pnprResult
    }
  }

  const run = await prepareMutationRun({ projects, maybeOpts, opts, detachReporter })
  const result = await settleInstall(runMutations(run), run.verifyLockfilePromise, detachReporter)

  reportVerifiedFileIntegrity(verifiedFileIntegritySince(verifiedFileIntegrityBaseline))

  return finishMutation(run, result)
}

function attachReporter (reporter: MutateModulesOptions['reporter'] | undefined): () => void {
  if ((reporter == null) || typeof reporter !== 'function') return () => {}
  streamParser.on('data', reporter)
  return () => {
    streamParser.removeListener('data', reporter)
  }
}

async function prepareMutationRun (base: MutationRunBase): Promise<MutationRun> {
  const { opts } = base
  const openedRun = await openMutationRun(base)
  const { ctx } = openedRun
  const { verifyLockfilePromise } = await startLockfileVerification(openedRun)

  await runPreResolutionHooks(opts, ctx)

  const forceResolutionFromHook = await shouldForceResolutionFromHook(opts, ctx)
  const pruneVirtualStore = shouldPruneVirtualStore(opts, ctx)

  if (!base.maybeOpts.ignorePackageManifest) {
    for (const { manifest, rootDir } of Object.values(ctx.projects)) {
      if (!manifest) {
        throw new Error(`No package.json found in "${rootDir}"`)
      }
    }
  }
  return {
    ...openedRun,
    verifyLockfilePromise,
    verifyLockfile: verifyLockfilePromise && (() => verifyLockfilePromise),
    forceResolutionFromHook,
    pruneVirtualStore,
  }
}

type OpenedMutationRun = Omit<MutationRun, 'forceResolutionFromHook' | 'pruneVirtualStore' | 'verifyLockfile' | 'verifyLockfilePromise'>

async function openMutationRun (base: MutationRunBase): Promise<OpenedMutationRun> {
  const { projects, opts } = base
  const allowBuild = createAllowBuildFunction(opts)

  const installsOnly = allMutationsAreInstalls(projects)
  const installsAndUninstallsOnly = installsOnly ||
    projects.every((project) => (
      (project.mutation === 'install' || project.mutation === 'installSome') &&
      !project.update && !project.updateMatching
    ) || project.mutation === 'uninstallSome')
  if (!installsOnly) opts.strictPeerDependencies = false
  const rootProjectManifest = opts.allProjects.find(({ rootDir }) => rootDir === opts.lockfileDir)?.manifest ??
    // When running install/update on a subset of projects, the root project might not be included,
    // so reading its manifest explicitly here.
    await safeReadProjectManifestOnly(opts.lockfileDir)

  const openedCtx = await getMutationContext(base)
  const scriptsOpts = createScriptsOptions(opts, openedCtx)
  const rootProjectPreinstallRan = await runRootProjectEarlyHooks(base, { ctx: openedCtx, rootProjectManifest, scriptsOpts })
  const ctx = await validateModulesAndReopenContext(opts, openedCtx, installsOnly)
  return {
    ...base,
    allowBuild,
    ctx,
    installsOnly,
    installsAndUninstallsOnly,
    rootProjectPreinstallRan,
    scriptsOpts,
  }
}

async function getMutationContext ({ projects, maybeOpts, opts }: MutationRunBase): Promise<PnpmContext> {
  const isUpdate = Boolean(
    (maybeOpts as { update?: boolean }).update ||
    projects.some((project) => ('update' in project && project.update) || ('updateMatching' in project && project.updateMatching))
  )
  const ctx = await getContext(isUpdate ? { ...opts, include: maybeOpts.include } : opts)
  if (isUpdate && !maybeOpts.include) {
    opts.include = resolveUpdateInclude(opts, ctx)
    ctx.include = opts.include
  }
  return ctx
}

interface DependencyGroupCliOptions {
  cliOptions?: Record<string, unknown>
  dev?: boolean
  production?: boolean
  optional?: boolean
  peer?: boolean
}

function resolveUpdateInclude (opts: StrictInstallOptions, ctx: PnpmContext): IncludedDependencies {
  const extraOpts = opts as DependencyGroupCliOptions
  const explicit = readExplicitDependencyGroups(extraOpts)
  const includesPeers = extraOpts.peer === true || (extraOpts.cliOptions != null && extraOpts.cliOptions.peer === true)
  const include = ctx.modulesFile != null
    ? {
      dependencies: ctx.include.dependencies || explicit.prod,
      devDependencies: ctx.include.devDependencies || explicit.dev,
      optionalDependencies: Boolean(ctx.include.optionalDependencies || explicit.optional) && !explicit.noOptional,
    }
    : {
      dependencies: true,
      devDependencies: !explicit.prod || explicit.dev,
      optionalDependencies: !explicit.noOptional,
    }
  return {
    ...include,
    ...(includesPeers ? { peerDependencies: true } : {}),
  }
}

function readExplicitDependencyGroups (extraOpts: DependencyGroupCliOptions): {
  dev: boolean
  prod: boolean
  optional: boolean
  noOptional: boolean
} {
  const cliOpts = extraOpts.cliOptions
  if (cliOpts == null) {
    return {
      dev: extraOpts.dev === true && extraOpts.production !== true,
      prod: extraOpts.production === true && extraOpts.dev !== true,
      optional: extraOpts.optional === true,
      noOptional: extraOpts.optional === false,
    }
  }
  return {
    dev: cliOpts.dev === true,
    prod: cliOpts.production === true || cliOpts.prod === true,
    optional: cliOpts.optional === true,
    noOptional: cliOpts.optional === false || (cliOpts.optional !== true && extraOpts.optional === false),
  }
}

function createScriptsOptions (opts: StrictInstallOptions, ctx: PnpmContext): RunLifecycleHooksConcurrentlyOptions {
  return {
    extraBinPaths: opts.extraBinPaths,
    extendNodePath: opts.extendNodePath,
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
}

/**
 * The root project's hooks run before the install touches `node_modules`,
 * which `validateModules` may purge. Returns whether the root project's
 * `preinstall` ran ahead of resolution.
 */
async function runRootProjectEarlyHooks (
  { projects, opts }: MutationRunBase,
  { ctx, rootProjectManifest, scriptsOpts }: {
    ctx: PnpmContext
    rootProjectManifest: ProjectManifest | null | undefined
    scriptsOpts: RunLifecycleHooksConcurrentlyOptions
  }
): Promise<boolean> {
  const rootHookOpts = {
    ...scriptsOpts,
    depPath: opts.lockfileDir,
    pkgRoot: opts.lockfileDir,
    rootModulesDir: ctx.rootModulesDir,
    wdBinDir: path.join(ctx.rootModulesDir, '.bin'),
    extraEnv: {
      ...scriptsOpts.extraEnv,
      ...await makeProjectNodePathOption({ modulesDir: ctx.rootModulesDir, rootDir: opts.lockfileDir }, opts),
    },
  }
  if (installRunsDevPreinstall(opts) && rootProjectManifest?.scripts?.[DEV_PREINSTALL]) {
    await runLifecycleHook(DEV_PREINSTALL, rootProjectManifest, rootHookOpts)
  }
  // The root project's `preinstall` runs before any dependency is resolved
  // or linked, so a guard such as `npx only-allow yarn` can still stop the
  // install. Its remaining stages run after linking, like every project's.
  const rootProjectPreinstallRan = rootProjectRunsPreinstallEarly(projects, opts)
  if (rootProjectPreinstallRan && rootProjectManifest?.scripts?.preinstall) {
    await runLifecycleHook('preinstall', rootProjectManifest, rootHookOpts)
  }
  return rootProjectPreinstallRan
}

async function validateModulesAndReopenContext (
  opts: StrictInstallOptions,
  ctx: PnpmContext,
  installsOnly: boolean
): Promise<PnpmContext> {
  if (opts.lockfileOnly || isCheckOnlyInstall(opts) || ctx.modulesFile == null) return ctx
  const { purged } = await validateModules(ctx.modulesFile, Object.values(ctx.projects), {
    forceNewModules: installsOnly,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    modulesDir: opts.modulesDir ?? 'node_modules',
    registriesByScope: opts.registriesByScope,
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
  return purged ? getContext(opts) : ctx
}

/**
 * Re-validate every entry in the lockfile against the policies the
 * resolver chain was built with.
 *
 * The verification is kicked off here, right after the lockfile is loaded,
 * but not awaited inline — it would otherwise block every later install
 * stage on per-entry registry round trips. Its synchronous prologue (cache
 * lookup, lockfile hashing, candidate collection) runs now against the
 * pristine lockfile, so the async fan-out reads a stable snapshot even
 * while the install mutates `ctx.wantedLockfile` concurrently. The verdict
 * is reconciled with the install in `settleInstall`: a failure aborts the
 * install even mid-flight, and an install that finishes first is held back
 * until the verdict arrives.
 *
 * The pending verdict is returned wrapped in an object, so that awaiting
 * this function does not await the verification.
 */
async function startLockfileVerification (
  { ctx, installsOnly, opts, projects }: OpenedMutationRun
): Promise<{ verifyLockfilePromise?: Promise<void> }> {
  if (willDelegateToPacquet(opts, ctx, installsOnly) || opts.trustLockfile) return {}
  const cacheActive = opts.cacheDir != null && opts.resolutionVerifiers.length > 0
  const wantedLockfilePath = cacheActive
    ? path.resolve(ctx.lockfileDir, await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    }))
    : undefined
  const verifyLockfilePromise = verifyLockfileResolutions(ctx.wantedLockfile, opts.resolutionVerifiers, {
    cacheDir: opts.cacheDir,
    lockfilePath: wantedLockfilePath,
    isReplaced: matchUpdateTargetsReplacedEverywhere(projects, ctx, opts.depth),
  })
  // Keep the rejection from going unhandled in the window before
  // `settleInstall` awaits the verdict — a preResolution hook or the
  // install kickoff below could throw and bail out before we get there.
  verifyLockfilePromise.catch(() => {})
  return { verifyLockfilePromise }
}

/**
 * Lockfile verification is skipped when we already know pacquet will run the
 * install: pacquet applies the same resolver-policy gate (port of
 * `verifyLockfileResolutions`) whether it materializes a frozen lockfile or
 * re-resolves from the manifests, so re-running it would duplicate the work —
 * and for `minimumReleaseAge` in strict mode each lockfile entry is an HTTP
 * probe.
 *
 * The predicate mirrors every short-circuit `tryFrozenInstall` checks
 * before reaching the pacquet branch: anything that would make it
 * return null, throw, or fall through to the JS path must keep
 * verification on. The optimistic `preferFrozenLockfile` path decides
 * whether to delegate later (based on `allProjectsAreUpToDate`), which
 * isn't known here — so verification still runs in that window, the
 * duplicate is bounded to it.
 */
function willDelegateToPacquet (opts: StrictInstallOptions, ctx: PnpmContext, installsOnly: boolean): boolean {
  return opts.runPacquet != null &&
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
      (ctx.patchedDepPathsStatus === 'up-to-date' &&
        ctx.existsNonEmptyWantedLockfile &&
        (opts.frozenLockfile === true || opts.frozenLockfileIfExists === true)) ||
      // Resolving install: pacquet (>= 0.11.7) re-resolves from the
      // manifests itself — applying the policy during fresh resolution —
      // so the existing lockfile entries verified here would just be
      // discarded. If a policy handler is active, keep resolution in pnpm
      // so violations can be returned to the command layer.
      (opts.saveLockfile && opts.runPacquet.supportsResolution && opts.frozenLockfile !== true && opts.nodeLinker !== 'hoisted' && opts.handleResolutionPolicyViolations == null)
    )
}

async function runPreResolutionHooks (opts: StrictInstallOptions, ctx: PnpmContext): Promise<void> {
  if (!opts.hooks.preResolution) return
  for (const preResolution of opts.hooks.preResolution) {
    // eslint-disable-next-line no-await-in-loop -- preResolution hooks run one after another, in the order they were registered
    await preResolution({
      currentLockfile: ctx.currentLockfile,
      wantedLockfile: ctx.wantedLockfile,
      existsCurrentLockfile: ctx.existsCurrentLockfile,
      existsNonEmptyWantedLockfile: ctx.existsNonEmptyWantedLockfile,
      lockfileDir: ctx.lockfileDir,
      storeDir: ctx.storeDir,
      registries: ctx.registriesByScope,
    })
  }
}

/**
 * Check if any custom resolvers want to force resolution for specific dependencies.
 * Skip this check when not saving the lockfile (e.g., during deploy) since there's no point
 * in forcing re-resolution if we're not going to persist the results.
 */
async function shouldForceResolutionFromHook (opts: StrictInstallOptions, ctx: PnpmContext): Promise<boolean> {
  const shouldCheckCustomResolverForceResolve =
    opts.hooks.customResolvers &&
    ctx.existsNonEmptyWantedLockfile &&
    !opts.frozenLockfile &&
    opts.saveLockfile
  if (!shouldCheckCustomResolverForceResolve) return false
  return checkCustomResolverForceResolve(
    opts.hooks.customResolvers!,
    ctx.wantedLockfile
  )
}

function shouldPruneVirtualStore (opts: StrictInstallOptions, ctx: PnpmContext): boolean {
  return !opts.enableGlobalVirtualStore && (ctx.modulesFile?.prunedAt && opts.modulesCacheMaxAge > 0
    ? cacheExpired(ctx.modulesFile.prunedAt, opts.modulesCacheMaxAge)
    : true
  )
}

// Reconcile the install with the lockfile verification that runs alongside
// it. The verification verdict is awaited first so it takes precedence and
// aborts as soon as it fails, even while the install is still in flight, so
// a rejected lockfile surfaces its own error rather than whatever the
// concurrent install happened to throw. Only once verification passes is the
// install's result (or error) surfaced. detachReporter mirrors the success
// path's cleanup so a long-lived process doesn't leak the stream listener on
// a rejected install.
async function settleInstall (
  install: Promise<InnerInstallResult>,
  verification: Promise<void> | undefined,
  detachReporter: () => void
): Promise<InnerInstallResult> {
  if (verification != null) {
    // Handle the install's eventual rejection up front so a fail-fast
    // verification throw below doesn't leave the still-running install
    // unhandled.
    install.catch(() => {})
  }
  try {
    if (verification != null) {
      await verification
    }
    return await install
  } catch (err) {
    detachReporter()
    throw err
  }
}

async function finishMutation (run: MutationRun, result: InnerInstallResult): Promise<MutateModulesResult> {
  const { ctx, opts } = run
  // The branch lockfiles become disposable only once the merge has been
  // written for good. An install that never saves a lockfile did not merge
  // them, and a `--dry-run` / `lockfileCheck` run only reports what it would
  // do — deleting them in either case drops resolutions no file is left
  // holding.
  if (
    opts.mergeGitBranchLockfiles && opts.useLockfile && opts.saveLockfile &&
    !isCheckOnlyInstall(opts)
  ) {
    await cleanGitBranchLockfiles(ctx.lockfileDir)
  }

  const ignoredBuilds = await settleIgnoredBuilds(run, result.ignoredBuilds)
  ignoredScriptsLogger.debug({
    packageNames: ignoredBuilds ? dedupePackageNamesFromIgnoredBuilds(ignoredBuilds) : [],
  })

  run.detachReporter()

  return {
    updatedCatalogs: result.updatedCatalogs,
    updatedProjects: result.updatedProjects,
    newLockfile: result.newLockfile,
    wantedLockfile: result.wantedLockfile,
    stats: result.stats ?? { added: 0, removed: 0, linkedToRoot: 0 },
    depsRequiringBuild: result.depsRequiringBuild,
    ignoredBuilds,
    resolutionPolicyViolations: result.resolutionPolicyViolations ?? [],
    dryRunResult: result.dryRunResult,
  }
}
