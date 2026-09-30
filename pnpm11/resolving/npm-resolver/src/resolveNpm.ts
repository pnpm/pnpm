import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { globalWarn } from '@pnpm/logger'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import type {
  PkgResolutionId,
  TarballResolution,
  WantedDependency,
  WorkspacePackages,
} from '@pnpm/resolving.resolver-base'
import type { DependencyManifest } from '@pnpm/types'
import semver from 'semver'

import { calcSpecifier } from './calcSpecifier.js'
import { NoMatchingVersionError } from './NoMatchingVersionError.js'
import { createRegistryTarballResolution, selectPackageRevision } from './packageRevision.js'
import { parseBareSpecifier, type RegistryPackageSpec } from './parseBareSpecifier.js'
import { pickPackageFromFetchedMeta, type PickPackageOptions } from './pickPackage.js'
import { createPickPackageOptions } from './pickPackageOptions.js'
import { warnOnceOnHeldBackUpdate } from './preferredVersionSelectors.js'
import { detectMinReleaseAgeViolation, latestAllowedByPolicy } from './releaseAgePolicy.js'
import type {
  NpmResolveResult,
  RegistrySpecRequest,
  ResolveFromNpmContext,
  ResolveFromNpmOptions,
  WorkspaceResolveResult,
} from './resolverTypes.js'
import { pickWithoutTrustDowngrade } from './trustChecks.js'
import {
  type LocalPackageResolutionOptions,
  pickMatchingLocalVersionOrNull,
  resolveFromLocalPackage,
  tryResolveFromWorkspace,
  tryResolveFromWorkspacePackages,
} from './workspaceResolution.js'

interface NpmSpecRequest extends RegistrySpecRequest {
  opts: ResolveFromNpmOptions
  /** The workspace packages this spec may resolve to, if any. */
  workspacePackages: WorkspacePackages | undefined
}

interface RegistryPick {
  meta: PackageMeta
  pickedPackage: PackageInRegistry
}

interface LockedPackage {
  id: PkgResolutionId
  manifest: DependencyManifest
  publishedAt?: string
  resolution: TarballResolution
}

export async function resolveNpm (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: ResolveFromNpmOptions
): Promise<NpmResolveResult | WorkspaceResolveResult | null> {
  const defaultTag = opts.defaultTag ?? 'latest'
  const registry = wantedDependency.alias
    ? pickRegistryForPackage(ctx.registriesByScope, wantedDependency.alias, wantedDependency.bareSpecifier)
    : ctx.registriesByScope.default
  if (wantedDependency.bareSpecifier?.startsWith('workspace:')) {
    if (wantedDependency.bareSpecifier.startsWith('workspace:.')) return null
    const resolvedFromWorkspace = tryResolveFromWorkspace(wantedDependency, {
      defaultTag,
      lockfileDir: opts.lockfileDir,
      projectDir: opts.projectDir,
      registry,
      workspacePackages: opts.workspacePackages,
      injectWorkspacePackages: opts.injectWorkspacePackages,
      update: Boolean(opts.update),
      updateRequested: Boolean(opts.updateRequested),
      saveWorkspaceProtocol: ctx.saveWorkspaceProtocol !== false ? ctx.saveWorkspaceProtocol : true,
      calcSpecifier: opts.calcSpecifier,
      rangeSpecStyle: opts.rangeSpecStyle,
    })
    if (resolvedFromWorkspace != null) {
      return resolvedFromWorkspace
    }
  }
  const spec = wantedDependency.bareSpecifier
    ? parseBareSpecifier(wantedDependency.bareSpecifier, wantedDependency.alias, defaultTag, registry)
    : defaultTagForAlias(wantedDependency.alias!, defaultTag)
  if (spec == null) return null
  return resolveNpmSpec({
    ctx,
    wantedDependency,
    opts,
    spec,
    registry,
    workspacePackages: workspacePackagesToTry(opts, spec),
  })
}

function workspacePackagesToTry (opts: ResolveFromNpmOptions, spec: RegistryPackageSpec): WorkspacePackages | undefined {
  const canKeepWorkspaceResolution = opts.currentPkg == null || opts.currentPkg.resolution.type === 'directory'
  return spec.revision == null && (!opts.updatePatches || canKeepWorkspaceResolution) && opts.alwaysTryWorkspacePackages !== false
    ? opts.workspacePackages
    : undefined
}

async function resolveNpmSpec (request: NpmSpecRequest): Promise<NpmResolveResult | WorkspaceResolveResult> {
  const { opts } = request
  const locked = await peekLockedPackageUnlessUpdating(request)
  if (locked != null && (opts.publishedBy == null || locked.publishedAt != null)) {
    return resolveFromLockedPackage(locked, opts)
  }
  const preferredWorkspaceResolution = resolveFromPreferredWorkspacePackage(request)
  if (preferredWorkspaceResolution != null) return preferredWorkspaceResolution

  const pickOptions = createPickPackageOptions(request)
  const registryPick = await pickFromRegistryOrWorkspace(request, pickOptions)
  if ('resolvedVia' in registryPick) return registryPick
  const { pickedPackage, rejectedVersions } = skipTrustDowngrades(request, registryPick, pickOptions)

  const latest = latestAllowedByPolicy(registryPick.meta, opts)
  const workspaceResolution = resolveFromWorkspacePackageMatchingPick(request, pickedPackage)
  if (workspaceResolution != null) {
    return { ...workspaceResolution, latest }
  }
  return createRegistryResolveResult(request, {
    meta: registryPick.meta,
    pickedPackage,
    trustDowngradesSkipped: rejectedVersions,
    latest,
    locked,
  })
}

// Fast path: if we have a current resolution with integrity, try to peek the manifest from the store.
// This avoids the expensive metadata fetch from the registry.
// We do this AFTER ensuring the spec is valid for this resolver to avoids hijacking other resolvers.
// If publishedBy is set (resolutionMode=time-based or minimumReleaseAge is configured), we only take
// the fast path when publishedAt is already known from the lockfile's `time:` block; otherwise we
// fall through to a registry fetch so the cutoff isn't computed from missing data.
async function peekLockedPackageUnlessUpdating ({ ctx, opts, spec }: NpmSpecRequest): Promise<LockedPackage | undefined> {
  if (
    opts.currentPkg == null ||
    opts.update ||
    opts.updatePatches ||
    opts.updateChecksums ||
    spec.revision != null ||
    opts.trustPolicy === 'no-downgrade'
  ) return undefined
  return peekLockedPackage(ctx, spec, opts.currentPkg)
}

function resolveFromLockedPackage (locked: LockedPackage, opts: ResolveFromNpmOptions): NpmResolveResult {
  return {
    id: locked.id,
    manifest: locked.manifest,
    resolution: locked.resolution,
    resolvedVia: 'npm-registry',
    publishedAt: locked.publishedAt,
    // Loose-mode bypass: a lockfile entry whose publishedAt sits
    // after the maturity cutoff would have been rejected at
    // resolver time, but the peek path skips the maturity check.
    // Report inline so the deps-resolver aggregator surfaces it
    // to the install command.
    policyViolation: detectMinReleaseAgeViolation({
      name: locked.manifest.name,
      version: locked.manifest.version,
      publishedAt: locked.publishedAt,
      resolution: locked.resolution,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
    }),
  }
}

// This runs *after* the store peek because a tag-specified dep whose only local copy is a
// prerelease reaches here with `update: false` (`wantedDepIsLocallyAvailable` ignores
// prereleases for tags, `pickMatchingLocalVersionOrNull` does not), and the peek must keep
// winning there. `update` is deliberately absent from the guard: that same helper forces it
// on for exactly these deps, so excluding it would make this block unreachable.
function resolveFromPreferredWorkspacePackage (request: NpmSpecRequest): WorkspaceResolveResult | undefined {
  const { opts, spec, workspacePackages } = request
  const isInjected = opts.injectWorkspacePackages === true || Boolean(request.wantedDependency.injected)
  if (
    opts.preferWorkspacePackages !== true ||
    workspacePackages == null ||
    !opts.projectDir ||
    opts.trustPolicy === 'no-downgrade' ||
    opts.updateChecksums ||
    isInjected
  ) return undefined
  const workspacePkgsMatchingName = spec.revision == null ? workspacePackages.get(spec.name) : undefined
  if (workspacePkgsMatchingName?.size !== 1) return undefined
  const localVersion = pickMatchingLocalVersionOrNull(workspacePkgsMatchingName, spec)
  if (localVersion == null) return undefined
  return resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, spec, localPackageResolutionOptions(request, {
    projectDir: opts.projectDir,
    hardLinkLocalPackages: false,
    update: Boolean(opts.update),
  }))
}

/**
 * Picks a version from the registry. When the registry has no match (or the
 * pick fails), a workspace package satisfying the spec is resolved instead.
 */
async function pickFromRegistryOrWorkspace (
  request: NpmSpecRequest,
  pickOptions: PickPackageOptions
): Promise<RegistryPick | WorkspaceResolveResult> {
  let pickResult!: { meta: PackageMeta, pickedPackage: PackageInRegistry | null }
  try {
    pickResult = await request.ctx.pickPackage(request.spec, pickOptions)
  } catch (err: unknown) {
    // When the registry doesn't have the package and the workspace has it
    // only at non-matching versions, the mismatch error (which lists the
    // available workspace versions) is more actionable than the raw 404.
    const resolvedFromWorkspace = resolveFromWorkspaceAfterRegistryMiss(request, (workspaceErr) =>
      (err as { code?: string }).code === 'ERR_PNPM_FETCH_404' && isNoMatchingVersionInsideWorkspace(workspaceErr))
    if (resolvedFromWorkspace != null) return resolvedFromWorkspace
    throw err
  }
  const { meta, pickedPackage } = pickResult
  if (pickedPackage != null) return { meta, pickedPackage }
  // Neither the registry nor the workspace has a matching version; the
  // workspace mismatch error carries the available local versions,
  // which is the actionable detail here.
  const resolvedFromWorkspace = resolveFromWorkspaceAfterRegistryMiss(request, isNoMatchingVersionInsideWorkspace)
  if (resolvedFromWorkspace != null) return resolvedFromWorkspace
  throw new NoMatchingVersionError({ wantedDependency: request.wantedDependency, packageMeta: meta, registry: request.registry })
}

function resolveFromWorkspaceAfterRegistryMiss (
  request: NpmSpecRequest,
  surfacesWorkspaceError: (workspaceErr: unknown) => boolean
): WorkspaceResolveResult | undefined {
  const { opts, workspacePackages } = request
  if (workspacePackages == null || !opts.projectDir) return undefined
  try {
    return tryResolveFromWorkspacePackages(workspacePackages, request.spec, localPackageResolutionOptions(request, {
      projectDir: opts.projectDir,
      hardLinkLocalPackages: opts.injectWorkspacePackages === true || request.wantedDependency.injected,
      update: false,
    }))
  } catch (workspaceErr) {
    if (surfacesWorkspaceError(workspaceErr)) throw workspaceErr
    return undefined
  }
}

function isNoMatchingVersionInsideWorkspace (err: unknown): boolean {
  return (err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION_INSIDE_WORKSPACE'
}

function skipTrustDowngrades (
  { ctx, opts, spec }: NpmSpecRequest,
  { meta, pickedPackage }: RegistryPick,
  pickOptions: PickPackageOptions
): { pickedPackage: PackageInRegistry, rejectedVersions: string[] } {
  if (opts.trustPolicy !== 'no-downgrade') return { pickedPackage, rejectedVersions: [] }
  return pickWithoutTrustDowngrade(meta, pickedPackage, {
    repick: (narrowedMeta) => pickPackageFromFetchedMeta(ctx, spec, pickOptions, narrowedMeta),
    trustCheck: {
      trustPolicyExclude: opts.trustPolicyExclude,
      trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
      ignoreMissingTimeField: ctx.ignoreMissingTimeField,
    },
  })
}

function resolveFromWorkspacePackageMatchingPick (
  request: NpmSpecRequest,
  pickedPackage: PackageInRegistry
): WorkspaceResolveResult | undefined {
  const { opts, spec } = request
  const workspacePkgsMatchingName = spec.revision == null ? request.workspacePackages?.get(pickedPackage.name) : undefined
  if (!workspacePkgsMatchingName || !opts.projectDir) return undefined
  const localOptions = localPackageResolutionOptions(request, {
    projectDir: opts.projectDir,
    hardLinkLocalPackages: opts.injectWorkspacePackages === true || request.wantedDependency.injected,
    update: Boolean(opts.update),
  })
  const matchedPkg = workspacePkgsMatchingName.get(pickedPackage.version)
  if (matchedPkg) {
    return resolveFromLocalPackage(matchedPkg, spec, localOptions)
  }
  const localVersion = pickMatchingLocalVersionOrNull(workspacePkgsMatchingName, spec)
  if (localVersion && (opts.preferWorkspacePackages || (semver.valid(localVersion) && semver.gte(localVersion, pickedPackage.version)))) {
    return resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, spec, localOptions)
  }
  return undefined
}

function localPackageResolutionOptions (
  { ctx, wantedDependency, opts }: NpmSpecRequest,
  target: Pick<LocalPackageResolutionOptions, 'projectDir' | 'hardLinkLocalPackages' | 'update'>
): LocalPackageResolutionOptions {
  return {
    wantedDependency,
    projectDir: target.projectDir,
    lockfileDir: opts.lockfileDir,
    hardLinkLocalPackages: target.hardLinkLocalPackages,
    update: target.update,
    updateRequested: Boolean(opts.updateRequested),
    saveWorkspaceProtocol: ctx.saveWorkspaceProtocol,
    calcSpecifier: opts.calcSpecifier,
    rangeSpecStyle: opts.rangeSpecStyle,
  }
}

function createRegistryResolveResult (
  { ctx, wantedDependency, opts, spec, registry }: NpmSpecRequest,
  picked: RegistryPick & {
    trustDowngradesSkipped: string[]
    latest: string | undefined
    locked: LockedPackage | undefined
  }
): NpmResolveResult {
  const { meta, pickedPackage, locked } = picked
  warnOnceOnTrustDowngradeFallback(ctx, spec.name, pickedPackage.version, picked.trustDowngradesSkipped)
  warnOnceOnHeldBackUpdate(ctx, opts, spec, meta, pickedPackage.version)
  const selectedPackage = selectPackageRevision(pickedPackage, spec, registry)
  const id = `${pickedPackage.name}@${pickedPackage.version}` as PkgResolutionId
  const resolution = createRegistryTarballResolution(selectedPackage.dist, registry)
  const normalizedBareSpecifier = opts.calcSpecifier
    ? spec.normalizedBareSpecifier ?? calcSpecifier({
      wantedDependency,
      spec,
      version: pickedPackage.version,
      defaultRangeSpecStyle: opts.rangeSpecStyle,
      isUpdate: Boolean(opts.updateRequested),
    })
    : undefined
  const publishedAt = meta.time?.[pickedPackage.version]
  // When the registry confirms the locked package, its manifest stays the one
  // the fast path reads. A registry whose metadata disagrees with the tarball
  // would otherwise rewrite the lockfile entry of a package nobody updated.
  const keepsLockedPackage = locked != null && id === locked.id && resolution.integrity === locked.resolution.integrity
  return {
    id,
    latest: picked.latest,
    manifest: keepsLockedPackage ? locked.manifest : selectedPackage,
    resolution,
    resolvedVia: 'npm-registry',
    publishedAt,
    normalizedBareSpecifier,
    policyViolation: detectMinReleaseAgeViolation({
      name: pickedPackage.name,
      version: pickedPackage.version,
      publishedAt,
      resolution,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
    }),
  }
}

const MAX_SKIPPED_VERSIONS_IN_WARNING = 5

function warnOnceOnTrustDowngradeFallback (
  ctx: Pick<ResolveFromNpmContext, 'warnedTrustDowngradeFallbacks'>,
  pkgName: string,
  pickedVersion: string,
  skippedVersions: string[]
): void {
  if (skippedVersions.length === 0) return
  const key = `${pkgName}@${pickedVersion}`
  if (ctx.warnedTrustDowngradeFallbacks.has(key)) return
  ctx.warnedTrustDowngradeFallbacks.add(key)
  let skipped = skippedVersions.slice(0, MAX_SKIPPED_VERSIONS_IN_WARNING).map((version) => `${pkgName}@${version}`).join(', ')
  if (skippedVersions.length > MAX_SKIPPED_VERSIONS_IN_WARNING) {
    skipped += ` and ${skippedVersions.length - MAX_SKIPPED_VERSIONS_IN_WARNING} more`
  }
  globalWarn(`Skipped trust downgrades rejected by trustPolicy: ${skipped}. Resolved ${pkgName}@${pickedVersion} instead.`)
}

function defaultTagForAlias (alias: string, defaultTag: string): RegistryPackageSpec {
  return {
    fetchSpec: defaultTag,
    name: alias,
    type: 'tag',
  }
}

/**
 * Reads the locked package's manifest from the store. Only a tarball
 * resolution pinned by integrity is looked up, and only while the stored
 * package is still the locked one and satisfies `spec`.
 */
async function peekLockedPackage (
  ctx: Pick<ResolveFromNpmContext, 'peekManifestFromStore'>,
  spec: RegistryPackageSpec,
  lockedPkg: NonNullable<ResolveFromNpmOptions['currentPkg']>
): Promise<LockedPackage | undefined> {
  const { resolution } = lockedPkg
  if (ctx.peekManifestFromStore == null || !('tarball' in resolution) || typeof resolution.integrity !== 'string') return undefined
  const manifest = await ctx.peekManifestFromStore({
    id: lockedPkg.id,
    integrity: resolution.integrity,
    name: lockedPkg.name,
    version: lockedPkg.version,
  })
  if (!manifest?.name || !manifest?.version) return undefined
  const satisfiesSpec =
    (spec.type !== 'range' || spec.fetchSpec === '*' || semver.satisfies(manifest.version, spec.fetchSpec, { loose: true })) &&
    (spec.type !== 'version' || manifest.version === spec.fetchSpec)
  if (`${manifest.name}@${manifest.version}` !== lockedPkg.id || !satisfiesSpec) return undefined
  return {
    id: lockedPkg.id,
    manifest,
    publishedAt: lockedPkg.publishedAt,
    resolution: resolution as TarballResolution,
  }
}
