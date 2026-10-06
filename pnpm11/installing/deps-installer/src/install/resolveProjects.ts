import { mergeCatalogs } from '@pnpm/catalogs.config'
import type { Catalogs } from '@pnpm/catalogs.types'
import { parseOverrides, type VersionOverride } from '@pnpm/config.parse-overrides'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { stageLogger } from '@pnpm/core-loggers'
import { createDependencyOverrider } from '@pnpm/hooks.read-package-hook'
import type { PnpmContext } from '@pnpm/installing.context'
import { resolveDependencies } from '@pnpm/installing.deps-resolver'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { getPreferredVersionsFromLockfileAndManifests } from '@pnpm/lockfile.preferred-versions'
import { createOverridesMapFromParsed } from '@pnpm/lockfile.settings-checker'
import type { PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { ProjectId, ReadPackageHook } from '@pnpm/types'
import { equals, isEmpty, map as mapValues, pipeWith } from 'ramda'

import { createInstallReadPackageHook } from './extendInstallOptions.js'
import { forgetResolutionsOfAllPrevWantedDeps } from './forgetResolutions.js'
import { omitPackagesNamed } from './getStaleOverrideTargets.js'
import {
  getUntrackedPnpmfileReadPackageHook,
  isCheckOnlyInstall,
  policyViolationKey,
  readPackageHookMayHaveChanged,
  setUntrackedPnpmfileReadPackageHook,
} from './installPredicates.js'
import type { ImporterToUpdate, InstallInContextOptions } from './mutationTypes.js'
import { warnOnStaleConvergenceOverrides } from './warnOnStaleConvergenceOverrides.js'

type ResolveDependenciesResult = Awaited<ReturnType<typeof resolveDependencies>>

/**
 * The resolved dependency graph, with the overrides it was resolved under.
 * The install steps after resolution narrow it in place.
 */
export type GraphResolution = ResolveDependenciesResult & {
  parsedOverrides: VersionOverride[]
}

type ResolveGraph = (
  catalogs: Catalogs | undefined,
  parsedOverrides: VersionOverride[],
  readPackageHook: ReadPackageHook | undefined,
  handleResolutionPolicyViolations?: InstallInContextOptions['handleResolutionPolicyViolations']
) => Promise<ResolveDependenciesResult>

export async function resolveProjects (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Promise<GraphResolution> {
  const untrackedPnpmfileReadPackageHook = getUntrackedPnpmfileReadPackageHook(opts.hooks)
  const untrackedReadPackageHookMayHaveChanged = readPackageHookMayHaveChanged(ctx.wantedLockfile, untrackedPnpmfileReadPackageHook)
  const preferredVersions = seedPreferredVersions(ctx, opts)
  const lockedPeersAreCurrent = ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION &&
    !opts.force &&
    !opts.needsFullResolution &&
    !ctx.lockfileHadConflicts &&
    !untrackedReadPackageHookMayHaveChanged
  const forceFullResolution = !lockedPeersAreCurrent ||
    !opts.currentLockfileIsUpToDate ||
    opts.dedupePeerDependents
  setUntrackedPnpmfileReadPackageHook(ctx.wantedLockfile, untrackedPnpmfileReadPackageHook)
  forgetRegeneratedLockfileFields(ctx, opts)

  const resolveGraph = createGraphResolver({ ctx, forceFullResolution, lockedPeersAreCurrent, opts, preferredVersions, projects })
  const resolution = await resolveGraphWithUpdatedCatalogOverrides(resolveGraph, opts)
  // Only a full resolution walks every manifest through the versions
  // overrider, making the collected declared ranges complete enough for the
  // staleness verdict; partial resolutions must stay silent to avoid false
  // positives from unseen ranges.
  if (opts.convergeDeclaredRanges != null && (forceFullResolution || opts.dedupe)) {
    await warnOnStaleConvergenceOverrides({
      convergeDeclaredRanges: opts.convergeDeclaredRanges,
      parsedOverrides: resolution.parsedOverrides,
      requestPackage: opts.storeController.requestPackage,
      lockfileDir: opts.lockfileDir,
      minimumReleaseAge: opts.minimumReleaseAge,
      minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    })
  }
  excludeUnincludedDependencies(resolution, { opts, projects })
  if (opts.skipRuntimes) {
    skipRuntimeDependencies(resolution, ctx)
  }

  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'resolution_done',
  })

  resolution.newLockfile = await applyAfterAllResolvedHooks(resolution.newLockfile, opts)

  if (opts.updateLockfileMinorVersion) {
    resolution.newLockfile.lockfileVersion = LOCKFILE_VERSION
  }
  return resolution
}

/**
 * Always seed preferred versions from the lockfile, even for update
 * mutations. Gating this on `update` would nullify the seed globally
 * during `pnpm up -r <pkg>`, so unrelated packages with open ranges
 * would lose their pins and re-resolve to newest-in-range
 * (pnpm/pnpm#10662). The targeted package still bumps: `updateRequested`
 * at the npm picker subtracts the lockfile-derived weight from its pins,
 * so the target re-resolves exactly as a fresh install would after its
 * lockfile entries were deleted.
 * Caller-supplied preferred versions (audit-fix vulnerability penalties)
 * layer on top of the seed per package name — replacing the seed with
 * them would unpin every unrelated package.
 */
function seedPreferredVersions (ctx: PnpmContext, opts: InstallInContextOptions): PreferredVersions {
  // Null-prototype merge target so a crafted package name (e.g. `__proto__`)
  // lands as a plain own key instead of invoking the prototype setter.
  const preferredVersions: PreferredVersions = Object.assign(
    Object.create(null),
    getPreferredVersionsFromLockfileAndManifests(
      omitPackagesNamed(ctx.wantedLockfile.packages, opts.staleOverrideTargets ?? new Set()),
      Object.values(ctx.projects).map(({ manifest }) => manifest),
      { catalogs: opts.catalogs, dedupe: opts.dedupe }
    )
  )
  for (const [pkgName, selectors] of Object.entries(opts.preferredVersions ?? {})) {
    preferredVersions[pkgName] = { ...preferredVersions[pkgName], ...selectors }
  }
  return preferredVersions
}

function forgetRegeneratedLockfileFields (ctx: PnpmContext, opts: InstallInContextOptions): void {
  // Ignore some fields when fixing lockfile, so these fields can be regenerated
  // and make sure it's up to date
  if (
    opts.fixLockfile &&
    (ctx.wantedLockfile.packages != null) &&
    !isEmpty(ctx.wantedLockfile.packages)
  ) {
    ctx.wantedLockfile.packages = mapValues(({ dependencies, optionalDependencies, resolution, deprecated, hasBin }) => ({
      // These fields are needed to avoid losing information of the locked dependencies if these fields are not broken
      // If these fields are broken, they will also be regenerated
      dependencies,
      optionalDependencies,
      resolution,
      deprecated,
      hasBin,
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
}

function createGraphResolver (
  { ctx, forceFullResolution, lockedPeersAreCurrent, opts, preferredVersions, projects }: {
    ctx: PnpmContext
    forceFullResolution: boolean
    lockedPeersAreCurrent: boolean
    opts: InstallInContextOptions
    preferredVersions: PreferredVersions
    projects: ImporterToUpdate[]
  }
): ResolveGraph {
  return async (
    catalogs,
    parsedOverrides,
    readPackageHook,
    handleResolutionPolicyViolations = opts.handleResolutionPolicyViolations
  ) => resolveDependencies(
    projects,
    {
      ...describeResolutionTarget(ctx, opts),
      ...describeResolutionPolicies(opts),
      catalogs,
      forceFullResolution,
      lockedPeersAreCurrent,
      hooks: {
        readPackage: readPackageHook,
      },
      overrideBareSpecifier: createDependencyOverrider(parsedOverrides, opts.lockfileDir),
      preferredVersions,
      handleResolutionPolicyViolations,
    }
  )
}

function describeResolutionTarget (ctx: PnpmContext, opts: InstallInContextOptions) {
  return {
    allowBuild: opts.allowBuild,
    currentLockfile: ctx.currentLockfile,
    dryRun: opts.lockfileOnly || isCheckOnlyInstall(opts),
    // The hoisted linker shares one tree and handles such an entry itself.
    hideAlienModules: opts.materializeAfterResolution && opts.nodeLinker !== 'hoisted',
    enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
    force: opts.force,
    staleOverrideTargets: opts.staleOverrideTargets,
    updateChecksums: opts.updateChecksums,
    ignoreScripts: opts.ignoreScripts,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? (opts.saveWorkspaceProtocol ? 0 : -1),
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    checkEnginesAgainstRootRuntime: opts.nodeVersion == null || opts.nodeVersionFromEnginesRuntime === true,
    pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
    preferWorkspacePackages: opts.preferWorkspacePackages,
    preferredVersionsByImporterId: opts.preferredVersionsByImporterId,
    preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
    registriesByScope: ctx.registriesByScope,
    registriesByPrefix: opts.registriesByPrefix,
    registryOptionsByUrl: opts.registryOptionsByUrl,
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    storeController: opts.storeController,
    globalVirtualStoreDir: opts.globalVirtualStoreDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
    wantedLockfile: ctx.wantedLockfile,
    workspacePackages: ctx.workspacePackages,
    patchedDependencies: opts.patchedDependencies,
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    supportedArchitectures: opts.supportedArchitectures,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    allProjectIds: Object.values(ctx.projects).map((project) => project.id),
  }
}

function describeResolutionPolicies (opts: InstallInContextOptions) {
  return {
    allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
    allowUnusedPatches: opts.allowUnusedPatches,
    autoInstallPeers: opts.autoInstallPeers,
    autoInstallPeersFromHighestMatch: opts.autoInstallPeersFromHighestMatch,
    defaultUpdateDepth: opts.depth,
    dedupeDirectDeps: opts.dedupeDirectDeps,
    dedupeInjectedDeps: opts.dedupeInjectedDeps,
    dedupePeerDependents: opts.dedupePeerDependents,
    dedupePeers: opts.dedupePeers,
    engineStrict: opts.engineStrict,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    resolutionMode: opts.resolutionMode,
    tag: opts.tag,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
    minimumReleaseAge: opts.minimumReleaseAge,
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    blockExoticSubdeps: opts.blockExoticSubdeps,
  }
}

async function resolveGraphWithUpdatedCatalogOverrides (
  resolveGraph: ResolveGraph,
  opts: InstallInContextOptions
): Promise<GraphResolution> {
  const resolution: GraphResolution = {
    ...await resolveGraph(opts.catalogs, opts.parsedOverrides, opts.readPackageHook),
    parsedOverrides: opts.parsedOverrides,
  }
  // `pnpm update` may bump catalog entries during resolution, while the
  // overrides that reference a catalog (e.g. `overrides: { foo: 'catalog:' }`)
  // were resolved against the pre-update catalog when the install options
  // were extended. When a bump changes such an override, resolve again with
  // the updated catalog so that the graph applies the override the lockfile
  // records.
  if (resolution.updatedCatalogs == null || isEmpty(opts.overrides ?? {})) return resolution
  const updatedCatalogsConfig = mergeCatalogs(opts.catalogs, resolution.updatedCatalogs)
  const overridesWithUpdatedCatalogs = parseOverrides(opts.overrides!, updatedCatalogsConfig)
  if (equals(createOverridesMapFromParsed(overridesWithUpdatedCatalogs), createOverridesMapFromParsed(resolution.parsedOverrides))) {
    return resolution
  }
  return resolveGraphWithUpdatedCatalogs(resolveGraph, {
    opts,
    parsedOverrides: overridesWithUpdatedCatalogs,
    resolution,
    updatedCatalogsConfig,
  })
}

async function resolveGraphWithUpdatedCatalogs (
  resolveGraph: ResolveGraph,
  { opts, parsedOverrides, resolution, updatedCatalogsConfig }: {
    opts: InstallInContextOptions
    parsedOverrides: VersionOverride[]
    resolution: GraphResolution
    updatedCatalogsConfig: Catalogs
  }
): Promise<GraphResolution> {
  const waitTillFirstResolutionFetchingsFinish = resolution.waitTillAllFetchingsFinish
  const handledViolations = new Set(resolution.resolutionPolicyViolations.map(policyViolationKey))
  const resolutionWithUpdatedCatalogs = await resolveGraph(
    updatedCatalogsConfig,
    parsedOverrides,
    createInstallReadPackageHook(opts, parsedOverrides),
    opts.handleResolutionPolicyViolations && (async (violations) => {
      const unhandled = violations.filter((violation) => !handledViolations.has(policyViolationKey(violation)))
      if (unhandled.length > 0) await opts.handleResolutionPolicyViolations!(unhandled)
    })
  )
  resolutionWithUpdatedCatalogs.newLockfile.overrides = createOverridesMapFromParsed(parsedOverrides)
  return {
    ...resolutionWithUpdatedCatalogs,
    parsedOverrides,
    updatedCatalogs: mergeCatalogs(resolution.updatedCatalogs, resolutionWithUpdatedCatalogs.updatedCatalogs),
    waitTillAllFetchingsFinish: async () => {
      await Promise.all([waitTillFirstResolutionFetchingsFinish(), resolutionWithUpdatedCatalogs.waitTillAllFetchingsFinish()])
    },
  }
}

function excludeUnincludedDependencies (
  resolution: GraphResolution,
  { opts, projects }: { opts: InstallInContextOptions, projects: ImporterToUpdate[] }
): void {
  const { include } = opts
  if (include.optionalDependencies && include.devDependencies && include.dependencies) return
  resolution.linkedDependenciesByProjectId = mapValues(
    (linkedDeps) => linkedDeps.filter((linkedDep) => isIncluded(include, { isDev: linkedDep.dev, isOptional: linkedDep.optional })),
    resolution.linkedDependenciesByProjectId ?? {}
  )
  for (const { id, manifest } of projects) {
    for (const [alias, depPath] of resolution.dependenciesByProjectId[id].entries()) {
      const isDependencyIncluded = resolution.dependenciesGraph[depPath] != null && isIncluded(include, {
        isDev: Object.hasOwn(manifest.devDependencies ?? {}, alias),
        isOptional: Object.hasOwn(manifest.optionalDependencies ?? {}, alias),
      })
      if (!isDependencyIncluded) {
        resolution.dependenciesByProjectId[id].delete(alias)
      }
    }
  }
}

function isIncluded (
  include: InstallInContextOptions['include'],
  { isDev, isOptional }: { isDev: boolean | undefined, isOptional: boolean | undefined }
): boolean {
  return !(
    isDev && !include.devDependencies ||
    isOptional && !(include.dependencies && include.optionalDependencies) ||
    !isDev && !isOptional && !include.dependencies
  )
}

/**
 * The lockfile filter (filterImporter) handles wantedLockfile-driven linking,
 * but the direct bin-linking path at the end of the install iterates
 * dependenciesByProjectId and only filters by ctx.skipped. Add runtime
 * depPaths there so that path skips them too.
 */
function skipRuntimeDependencies (resolution: GraphResolution, ctx: PnpmContext): void {
  for (const id of Object.keys(resolution.dependenciesByProjectId) as ProjectId[]) {
    for (const [alias, depPath] of resolution.dependenciesByProjectId[id].entries()) {
      if (depPath.includes('@runtime:')) {
        ctx.skipped.add(depPath)
        resolution.dependenciesByProjectId[id].delete(alias)
      }
    }
  }
}

async function applyAfterAllResolvedHooks (newLockfile: LockfileObject, opts: InstallInContextOptions): Promise<LockfileObject> {
  return ((opts.hooks?.afterAllResolved) != null)
    ? await pipeWith(async (hook, result) => hook(await result), opts.hooks.afterAllResolved as any)(newLockfile) as LockfileObject // eslint-disable-line @typescript-eslint/no-explicit-any -- pipeWith cannot type a list of hooks that each take the previous hook's result
    : newLockfile
}
