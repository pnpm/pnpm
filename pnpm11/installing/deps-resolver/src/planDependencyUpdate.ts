import { parseBareSpecifier } from '@pnpm/resolving.npm-resolver'
import type { PreferredVersions } from '@pnpm/resolving.resolver-base'

import type { WantedDependency } from './getWantedDependencies.js'
import type {
  ExtendedWantedDependency,
  ResolutionContext,
  ResolvedDependenciesOptions,
  ResolveDependencyOptions,
  UpdateMatchingFunction,
} from './resolutionTypes.js'
import { wantedDepIsLocallyAvailable } from './wantedDepIsLocallyAvailable.js'

export interface DependencyUpdatePlan {
  update: boolean
  updateDepth: number
  updateRequested: boolean
  updateShouldContinue: boolean
}

export function planDependencyUpdate (
  ctx: ResolutionContext,
  options: ResolvedDependenciesOptions,
  extendedWantedDep: ExtendedWantedDependency
): DependencyUpdatePlan {
  const { infoFromLockfile, wantedDependency } = extendedWantedDep
  const updateDepth = typeof wantedDependency.updateDepth === 'number'
    ? wantedDependency.updateDepth
    : options.updateDepth
  const updateShouldContinue = options.currentDepth <= updateDepth
  const updateRequested = updateShouldContinue && isUpdateRequested(ctx, options.updateMatching, extendedWantedDep)
  const update = updateRequested ||
    infoFromLockfile?.dependencyLockfile == null ||
    isLinkedFromWorkspace(ctx, wantedDependency) ||
    ctx.updatedSet.has(infoFromLockfile.name!)
  return { update, updateDepth, updateRequested, updateShouldContinue }
}

function isUpdateRequested (
  ctx: ResolutionContext,
  updateMatching: UpdateMatchingFunction | undefined,
  { infoFromLockfile, wantedDependency }: ExtendedWantedDependency
): boolean {
  if (updateMatching == null) return true
  if (infoFromLockfile?.name != null) {
    return updateMatching(infoFromLockfile.name, infoFromLockfile.version)
  }
  // A changed specifier forgets the edge's lockfile reference before
  // resolution (e.g. `pnpm audit --fix` widening a vulnerable pin), so
  // the target would otherwise lose its updateRequested status — and
  // keep its seeded lockfile pins — at the very moment it is being
  // updated. Fall back to matching by the wanted dependency itself.
  return wantedDependencyMatchesUpdateTarget(ctx, updateMatching, wantedDependency)
}

function isLinkedFromWorkspace (ctx: ResolutionContext, wantedDependency: WantedDependency): boolean {
  return Boolean(
    (ctx.workspacePackages != null) &&
    ctx.linkWorkspacePackagesDepth !== -1 &&
    wantedDepIsLocallyAvailable(
      ctx.workspacePackages,
      wantedDependency,
      { defaultTag: ctx.defaultTag, registry: ctx.registriesByScope.default }
    )
  )
}

function getUpdateMode (
  wantedDependency: WantedDependency,
  update: boolean,
  updateToLatest: boolean | undefined
): ResolveDependencyOptions['update'] {
  const mustKeepSpecifier = wantedDependency.updateToLatestAllowed === false
  // No update mode narrows a `workspace:` range: the workspace picker takes the newest local
  // version whenever an update is requested at all. A specifier that has to be kept can only be
  // kept by resolving this edge the way a plain install would.
  const mustKeepWorkspaceRange = mustKeepSpecifier &&
    wantedDependency.bareSpecifier?.startsWith('workspace:') === true
  if (!update || mustKeepWorkspaceRange) return false
  return updateToLatest && !mustKeepSpecifier ? 'latest' : 'compatible'
}

export function getResolveDependencyOptions (
  ctx: ResolutionContext,
  { extendedWantedDep, options, plan, preferredVersions }: {
    extendedWantedDep: ExtendedWantedDependency
    options: ResolvedDependenciesOptions
    plan: DependencyUpdatePlan
    preferredVersions: PreferredVersions
  }
): ResolveDependencyOptions {
  return {
    currentDepth: options.currentDepth,
    parentPkg: options.parentPkg,
    parentPkgAliases: options.parentPkgAliases,
    preferredVersions,
    currentPkg: extendedWantedDep.infoFromLockfile ?? undefined,
    preferredVersion: extendedWantedDep.preferredVersion,
    pickLowestVersion: options.pickLowestVersion,
    prefix: options.prefix,
    proceed: extendedWantedDep.proceed || plan.updateShouldContinue || ctx.updatedSet.size > 0,
    publishedBy: options.publishedBy,
    update: getUpdateMode(extendedWantedDep.wantedDependency, plan.update, options.updateToLatest),
    updatePatches: options.updatePatches,
    updateChecksums: ctx.updateChecksums,
    updateDepth: plan.updateDepth,
    updateRequested: plan.updateRequested,
    supportedArchitectures: options.supportedArchitectures,
    parentIds: options.parentIds,
    rangeSpecStyle: options.rangeSpecStyle,
  }
}

/**
 * Whether a wanted dependency without a lockfile reference matches the
 * update target by package name. The name is parsed from the bare
 * specifier, so an `npm:` alias matches by the real package name it
 * installs — `foo@npm:bar@^4` matches an update target of `bar`, not
 * `foo`. Exotic specifiers the npm parser rejects fall back to the alias.
 */
function wantedDependencyMatchesUpdateTarget (
  ctx: ResolutionContext,
  updateMatching: UpdateMatchingFunction,
  wantedDependency: WantedDependency
): boolean {
  const { alias, bareSpecifier } = wantedDependency
  const spec = alias && bareSpecifier
    ? parseBareSpecifier(bareSpecifier, alias, ctx.defaultTag ?? 'latest', ctx.registriesByScope.default)
    : null
  const name = spec?.name ?? alias
  return name != null && updateMatching(name, undefined)
}
