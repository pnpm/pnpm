import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { createPackageVersionPolicyOrThrow, getPublishedByPolicy } from '@pnpm/config.version-policy'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { packageManifestLogger } from '@pnpm/core-loggers'
import type { PnpmContext } from '@pnpm/installing.context'
import { isEmptyLockfile, type LockfileObject } from '@pnpm/lockfile.fs'
import type { ChangedField } from '@pnpm/lockfile.settings-checker'
import { allProjectsAreUpToDate } from '@pnpm/lockfile.verification'
import type { ProjectRootDir } from '@pnpm/types'

import { isCheckOnlyInstall, isSettingsField } from './installPredicates.js'
import { type InstallChecksums, type LockfileSettingsState, type StagedAdditions, upToDateCheckOptions } from './lockfileState.js'
import type { MutationRun } from './mutationTypes.js'
import type { AddedManifests } from './tryAddLockedVersions.js'
import { tryComposeFastUpdates } from './tryComposeFastUpdates.js'
import { hasChangedProjectSpecifiers } from './tryFastUpdateImporters.js'
import { tryFastUpdateLockfile } from './tryFastUpdateLockfile.js'
import { warnUnusedPatches } from './tryFastUpdatePatchedDependencies.js'
import { verifyLockfileResolutions } from './verifyLockfileResolutions.js'

const COMPOSABLE_CHANGED_FIELDS = new Set<ChangedField>([
  'catalogs',
  'ignoredOptionalDependencies',
  'overrides',
  'patchedDependencies',
])

export interface FastUpdateInput {
  additions: StagedAdditions
  checksums: InstallChecksums
  settings: LockfileSettingsState
}

type ContextProject = PnpmContext['projects'][ProjectRootDir]

type LockfileUpToDateCheck = (lockfile: LockfileObject) => Promise<boolean>

type ComposeFastUpdatesOptions = Parameters<typeof tryComposeFastUpdates>[1]

interface DriftPlan {
  contextProjects: ContextProject[]
  drift: Required<ComposeFastUpdatesOptions['drift']>
  hasAsyncDrift: boolean
  composableDrift: boolean
}

/**
 * Rewrites the wanted lockfile for the drift a resolution-free update can
 * absorb. On success the staged `installSome` manifests are committed to the
 * context, and the result tells whether overrides were rewritten. `null` when
 * the lockfile was left for the resolver.
 */
export async function tryFastLockfileUpdate (
  run: MutationRun,
  input: FastUpdateInput
): Promise<{ didFastUpdateOverrides: boolean } | null> {
  const { ctx } = run
  const plan = planDriftUpdate(run, input)
  if (!plan.composableDrift || !canComposeOntoLockfile(run, { ...input, hasAsyncDrift: plan.hasAsyncDrift })) return null
  await run.verifyLockfilePromise
  const isLockfileUpToDate: LockfileUpToDateCheck = async (lockfile) =>
    allProjectsAreUpToDate(plan.contextProjects, upToDateCheckOptions(run.opts, { ctx, wantedLockfile: lockfile }))
  // Built only for the rewrites that consult the resolver: deriving the
  // policies rejects a malformed pattern, which is the resolver's error
  // to report on a run that has no use for them.
  const rewriteOptions = plan.hasAsyncDrift ? createRewriteOptions(run, isLockfileUpToDate) : undefined
  const updated = await tryFastUpdateLockfile(ctx.wantedLockfile, {
    update: async (candidate) => tryComposeFastUpdates(candidate, createComposeOptions(run, { input, plan, rewriteOptions })),
    isLockfileUpToDate,
    verifyLockfile: (lockfile) => verifyLockfileResolutions(lockfile, []),
  })
  if (!updated) return null
  ctx.wantedLockfileIsModified = true
  commitAddedManifests(ctx, input.additions.addedManifests!)
  // Only the committed candidate is worth reporting on: a rewrite the
  // freshness gates reject is followed by the resolution, which reports
  // it itself.
  warnUnusedPatches(ctx.wantedLockfile, {
    patchedDependencies: input.checksums.patchedDependencies,
    allowUnusedPatches: run.opts.allowUnusedPatches,
  })
  return { didFastUpdateOverrides: plan.drift.overrides }
}

function planDriftUpdate ({ ctx, opts }: MutationRun, { additions, settings }: FastUpdateInput): DriftPlan {
  const { changedLockfileSettings } = settings
  const contextProjects = Object.values(ctx.projects).map((project) => {
    const added = additions.addedManifests?.get(project.rootDir)
    return added == null ? project : { ...project, ...added }
  })
  // A catalog move can change the effective value of an override whose
  // configured value is a `catalog:` reference — an effect no catalog
  // rewrite can express — so catalog drift under such an override goes to
  // the resolver. Override drift alone composes: override values are
  // compared catalog-resolved, so a settled `catalog:` override shows no
  // drift and only the genuinely changed entries reach the override
  // rewrite.
  const overridesUseCatalogs = Object.values(configuredOverrides(opts))
    .some((specifier) => parseCatalogProtocol(specifier) != null)
  const hasAsyncDrift = changedLockfileSettings.includes('catalogs') ||
    changedLockfileSettings.includes('overrides')
  const allChangedFieldsAreComposable =
    !(changedLockfileSettings.includes('catalogs') && overridesUseCatalogs) &&
    changedLockfileSettings.every((field) =>
      isSettingsField(field) || COMPOSABLE_CHANGED_FIELDS.has(field))
  // A changed field nothing can absorb forces a resolution, so the
  // workspace-wide specifier scan would be wasted.
  const hasChangedSpecifiers = allChangedFieldsAreComposable &&
    hasChangedProjectSpecifiers(ctx.wantedLockfile, contextProjects, opts.pruneLockfileImporters)
  return {
    contextProjects,
    drift: {
      importers: hasChangedSpecifiers,
      ignoredOptionalDependencies: changedLockfileSettings.includes('ignoredOptionalDependencies'),
      patchedDependencies: changedLockfileSettings.includes('patchedDependencies'),
      settings: changedLockfileSettings.some(isSettingsField),
      catalogs: changedLockfileSettings.includes('catalogs'),
      overrides: changedLockfileSettings.includes('overrides'),
    },
    hasAsyncDrift,
    composableDrift: (hasChangedSpecifiers || changedLockfileSettings.length > 0) && allChangedFieldsAreComposable,
  }
}

// Typed as required, but a caller that passes it through from its own
// optional config leaves it undefined.
function configuredOverrides (opts: MutationRun['opts']): Record<string, string> {
  return opts.overrides ?? {}
}

function canComposeOntoLockfile (
  { ctx, forceResolutionFromHook, installsAndUninstallsOnly, opts }: MutationRun,
  { additions, checksums, hasAsyncDrift }: FastUpdateInput & { hasAsyncDrift: boolean }
): boolean {
  return !checksums.frozenLockfile &&
    // `pnpm fetch` installs from the lockfile alone; with its empty
    // manifests every recorded dependency would read as removed.
    !opts.ignorePackageManifest &&
    installsAndUninstallsOnly &&
    additions.addedManifests != null &&
    !isCheckOnlyInstall(opts) &&
    opts.preferFrozenLockfile &&
    opts.useLockfile &&
    opts.saveLockfile &&
    opts.runPacquet == null &&
    !opts.fixLockfile &&
    !opts.dedupe &&
    !opts.updateChecksums &&
    !opts.force &&
    !opts.forceFullResolution &&
    !forceResolutionFromHook &&
    // A pnpmfile's `readPackage` hook is safe here — `getContext` applies
    // it to every project manifest and a changed pnpmfile surfaces as
    // `pnpmfileChecksum` drift. A hook the checksum cannot vouch for — a
    // programmatic one, or one from the checksum-excluded global pnpmfile
    // — keeps forcing the resolver.
    !checksums.untrackedReadPackageHookMayHaveChanged &&
    !checksums.pnpmfileChecksumIgnored &&
    !opts.hooks.preResolution?.length &&
    !opts.hooks.afterAllResolved?.length &&
    opts.hooks.customResolvers == null &&
    !ctx.lockfileHadConflicts &&
    // A lockfile that disagrees with its own `patchedDependencies`, or that could
    // not be checked, needs the resolver to rewrite its dependency paths;
    // composing onto it would carry the stale hashes forward.
    ctx.patchedDepPathsStatus === 'up-to-date' &&
    ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION &&
    !isEmptyLockfile(ctx.wantedLockfile) &&
    // `time` records publish dates for the importers' direct dependencies
    // and is pruned back to them whenever the lockfile is written, so a
    // rewrite that introduces no new version needs no maintenance of it.
    // The resolver-consulting rewrites do introduce versions, whose dates
    // only a resolution can record.
    (ctx.wantedLockfile.time == null || !hasAsyncDrift)
}

function createRewriteOptions ({ ctx, opts }: MutationRun, isLockfileUpToDate: LockfileUpToDateCheck) {
  return {
    ...getPublishedByPolicy(opts),
    isLockfileUpToDate,
    lockfileDir: opts.lockfileDir,
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    readPackageHook: opts.readPackageHook,
    registriesByScope: ctx.registriesByScope,
    requestPackage: opts.storeController.requestPackage,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude
      ? createPackageVersionPolicyOrThrow(opts.trustPolicyExclude, 'trustPolicyExclude')
      : undefined,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
  }
}

function createComposeOptions (
  { ctx, opts }: MutationRun,
  { input, plan, rewriteOptions }: {
    input: FastUpdateInput
    plan: DriftPlan
    rewriteOptions: ReturnType<typeof createRewriteOptions> | undefined
  }
): ComposeFastUpdatesOptions {
  const { contextProjects, drift } = plan
  return {
    drift,
    projects: contextProjects,
    workspacePackages: ctx.workspacePackages,
    resolutionPicksLowest: input.additions.resolutionPicksLowest,
    pruneLockfileImporters: opts.pruneLockfileImporters,
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
    patchedDependencies: {
      patchedDependencies: input.checksums.patchedDependencies,
      allowUnusedPatches: opts.allowUnusedPatches,
    },
    settings: {
      changedSettings: input.settings.changedLockfileSettings.filter(isSettingsField),
      projects: contextProjects,
      settings: input.settings.wantedLockfileSettings,
      workspacePackages: ctx.workspacePackages,
    },
    catalogs: rewriteOptions && {
      ...rewriteOptions,
      catalogs: opts.catalogs,
      overrides: configuredOverrides(opts),
      parsedOverrides: opts.parsedOverrides,
    },
    overrides: rewriteOptions && {
      ...rewriteOptions,
      overrides: input.settings.overridesMap,
      parsedOverrides: opts.parsedOverrides,
    },
  }
}

function commitAddedManifests (ctx: PnpmContext, addedManifests: Map<ProjectRootDir, AddedManifests>): void {
  for (const [rootDir, added] of addedManifests) {
    const project = ctx.projects[rootDir]
    project.manifest = added.manifest
    if (added.originalManifest != null) {
      project.originalManifest = added.originalManifest
    }
    packageManifestLogger.debug({
      prefix: rootDir,
      updated: added.manifest,
    })
  }
}
