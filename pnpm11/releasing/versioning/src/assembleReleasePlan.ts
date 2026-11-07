import { PnpmError } from '@pnpm/error'
import type { VersioningSettings } from '@pnpm/types'
import { compare, diff, satisfies } from 'semver'

import { applyEpicBandVersions, enforceEpicBands, epicRebaseFloor, type ResolvedEpic } from './epics.js'
import type { ChangeIntent, IntentBumpType, ReleaseBumpType } from './intents.js'
import { buildConsumptionIndex, type Ledger, type PackageConsumption } from './ledger.js'
import { assertInternalDepsUseWorkspaceProtocol, type InternalDep, type Participant } from './participants.js'
import type { WorkspaceProject } from './projectRefs.js'
import {
  BUMP_ORDER,
  computeNewVersion,
  materializeWorkspaceRange,
  maxBumpType,
  nextPrereleaseNumber,
  stableTarget,
} from './versionMath.js'
import { resolveIntents, resolveWorkspaceVersioning } from './workspaceVersioning.js'

export {
  indexProjectRefs,
  isDirRef,
  privateProjectDirs,
  type ProjectRefIndex,
  toProjectDir,
  type WorkspaceProject,
} from './projectRefs.js'
export {
  checkVersioningInvariants,
  type CheckVersioningInvariantsOptions,
  type VersioningInvariantViolation,
} from './versioningInvariants.js'
export { materializeWorkspaceRange } from './versionMath.js'


export type ReleaseCause = 'intent' | 'dependencies' | 'fixed' | 'epic'

export interface DependencyUpdate {
  name: string
  newVersion: string
}

export interface PlannedRelease {
  name: string
  /** Workspace-relative project directory — the engine's unit of identity. */
  dir: string
  rootDir: string
  currentVersion: string
  newVersion: string
  bumpType: ReleaseBumpType
  /**
   * The intent files this release consumes for this package: the pending ones,
   * plus — when the release graduates the package off a lane — the
   * ones the ledger recorded against the lane's prerelease versions.
   */
  intents: ChangeIntent[]
  dependencyUpdates: DependencyUpdate[]
  causes: ReleaseCause[]
}

export interface ReleasePlan {
  releases: PlannedRelease[]
}

export interface AssembleReleasePlanOptions {
  workspaceDir: string
  projects: WorkspaceProject[]
  intents: ChangeIntent[]
  ledger: Ledger
  versioning?: VersioningSettings
  /**
   * Workspace-relative directories of the projects selected with --filter.
   * The plan is narrowed to the selected packages' portion of the pending
   * work, expanded with their fixed-group companions and range-invalidated
   * dependents.
   */
  filter?: Set<string>
  /**
   * When set, every planned release gets the version `0.0.0-<suffix>` instead
   * of the computed one, matching snapshot releases.
   */
  snapshotSuffix?: string
  /**
   * Enforce that every internal production dependency uses the `workspace:`
   * protocol — a prerequisite for actually releasing. The release path
   * (`pnpm version -r`) sets this; read-only callers (`pnpm change status`)
   * leave it off so a diagnostic never fails on an unmigrated dependency.
   */
  enforceWorkspaceProtocol?: boolean
  /**
   * Directories whose current manifest version the registry does not have.
   * Their first release publishes that version verbatim, so the pending change
   * intents bump it only from the next release. Resolved by the command layer's
   * registry probe. Fixed-group sharing and epic band re-basing still override
   * it, since a package cannot opt out of those workspace-wide version rules.
   */
  unpublishedDirs?: Set<string>
}

export function assembleReleasePlan (opts: AssembleReleasePlanOptions): ReleasePlan {
  const { refs, participants, lanesByDir, fixedGroups, epics } = resolveWorkspaceVersioning(opts)
  const intentBumps = resolveIntents(opts.intents, refs, participants)
  if (opts.enforceWorkspaceProtocol) {
    assertInternalDepsUseWorkspaceProtocol(participants)
  }
  const consumptionOf = buildConsumptionIndex(opts.ledger, refs.nameToDirs)

  const ctx: AssembleContext = { participants, lanesByDir, fixedGroups, epics, intentBumps, consumptionOf, opts }
  return assembleUntilSelectionIsClosed(ctx, opts.filter)
}

/**
 * Re-assembles the plan with the selection widened by every package the
 * previous plan released, until the selection stops growing.
 */
function assembleUntilSelectionIsClosed (ctx: AssembleContext, filter: Set<string> | undefined): ReleasePlan {
  let selection = filter
  for (;;) {
    const plan = assemble(ctx, selection)
    if (selection == null) return plan
    const expanded = new Set(selection)
    for (const release of plan.releases) {
      expanded.add(release.dir)
    }
    if (expanded.size === selection.size) return plan
    selection = expanded
  }
}

interface BumpState {
  bumpType: ReleaseBumpType
  causes: Set<ReleaseCause>
  dependencyUpdates: Map<string, string>
}

interface AssembleContext {
  participants: Map<string, Participant>
  lanesByDir: Map<string, string>
  fixedGroups: string[][]
  epics: ResolvedEpic[]
  /** Per intent id: the participant dirs it releases and their bump types. */
  intentBumps: Map<string, Map<string, IntentBumpType>>
  consumptionOf: (dir: string) => PackageConsumption
  opts: AssembleReleasePlanOptions
}

/** The working state of one plan assembly for a given selection. */
interface PlanState {
  ctx: AssembleContext
  pendingByDir: Map<string, ChangeIntent[]>
  laneConsumedByDir: Map<string, ChangeIntent[]>
  bumps: Map<string, BumpState>
  newVersions: Map<string, string>
}

function assemble (ctx: AssembleContext, selection: Set<string> | undefined): ReleasePlan {
  const plan: PlanState = {
    ctx,
    pendingByDir: collectPendingIntents(ctx),
    laneConsumedByDir: collectLaneConsumedIntents(ctx),
    bumps: new Map(),
    newVersions: new Map(),
  }
  const isSelected = (dir: string): boolean => selection == null || selection.has(dir)
  seedPendingIntentBumps(plan, isSelected)
  seedGraduationBumps(plan, isSelected)

  for (let changed = true; changed;) {
    computeVersions(plan)
    changed = propagateBumps(plan)
  }
  computeVersions(plan)

  const releases = buildReleases(plan)
  assertNoDuplicateReleaseIdentity(releases)
  if (ctx.opts.snapshotSuffix == null) {
    enforceEpicBands(ctx.epics, ctx.participants, plan.newVersions)
    enforceMaxBump(releases, ctx.opts.versioning)
  }

  return { releases }
}

interface BumpRequest {
  dir: string
  bumpType: ReleaseBumpType
  cause: ReleaseCause
}

/** Raises a package's planned bump to at least `bumpType`; returns whether the bump grew. */
function bumpAtLeast (bumps: Map<string, BumpState>, { dir, bumpType, cause }: BumpRequest): boolean {
  const existing = bumps.get(dir)
  if (existing == null) {
    bumps.set(dir, { bumpType, causes: new Set([cause]), dependencyUpdates: new Map() })
    return true
  }
  existing.causes.add(cause)
  if (BUMP_ORDER[bumpType] > BUMP_ORDER[existing.bumpType]) {
    existing.bumpType = bumpType
    return true
  }
  return false
}

function intentBumpFor (ctx: AssembleContext, intent: ChangeIntent, dir: string): IntentBumpType | undefined {
  return ctx.intentBumps.get(intent.id)?.get(dir)
}

function seedPendingIntentBumps (plan: PlanState, isSelected: (dir: string) => boolean): void {
  for (const [dir, pending] of plan.pendingByDir.entries()) {
    if (!isSelected(dir)) continue
    const direct = maxBumpType(pending.map((intent) => intentBumpFor(plan.ctx, intent, dir)))
    if (direct != null) {
      bumpAtLeast(plan.bumps, { dir, bumpType: direct, cause: 'intent' })
    }
  }
}

/**
 * A package that left its lane releases the accumulated stable
 * version even when no new intents are pending.
 */
function seedGraduationBumps (plan: PlanState, isSelected: (dir: string) => boolean): void {
  for (const [dir, laneConsumed] of plan.laneConsumedByDir.entries()) {
    if (!isSelected(dir)) continue
    if (plan.ctx.lanesByDir.has(dir) || laneConsumed.length === 0) continue
    const graduated = maxBumpType(laneConsumed.map((intent) => intentBumpFor(plan.ctx, intent, dir)))
    if (graduated != null) {
      bumpAtLeast(plan.bumps, { dir, bumpType: graduated, cause: 'intent' })
    }
  }
}

function cumulativeBump (plan: PlanState, dir: string, planned: ReleaseBumpType): ReleaseBumpType {
  const laneConsumed = plan.laneConsumedByDir.get(dir) ?? []
  return maxBumpType([planned, ...laneConsumed.map((intent) => intentBumpFor(plan.ctx, intent, dir))]) ?? planned
}

function computeVersions (plan: PlanState): void {
  const { ctx, bumps, newVersions } = plan
  newVersions.clear()
  for (const [dir, pkgState] of bumps.entries()) {
    newVersions.set(dir, computeNewVersion(ctx.participants.get(dir)!.currentVersion, pkgState.bumpType, {
      laneTag: ctx.lanesByDir.get(dir),
      cumulativeBump: cumulativeBump(plan, dir, pkgState.bumpType),
      firstRelease: ctx.opts.unpublishedDirs?.has(dir) ?? false,
    }))
  }
  applyFixedGroupVersions(plan)
  applyEpicBandVersions({ participants: ctx.participants, state: bumps, newVersions, epics: ctx.epics, lanesByDir: ctx.lanesByDir })
}

function applyFixedGroupVersions (plan: PlanState): void {
  for (const group of plan.ctx.fixedGroups) {
    const bumpedMembers = group.filter((dir) => plan.bumps.has(dir))
    if (bumpedMembers.length === 0) continue
    const sharedVersion = fixedGroupSharedVersion(plan, group, bumpedMembers)
    for (const dir of bumpedMembers) {
      plan.newVersions.set(dir, sharedVersion)
    }
  }
}

function fixedGroupSharedVersion (plan: PlanState, group: string[], bumpedMembers: string[]): string {
  const { participants, lanesByDir } = plan.ctx
  const groupBump = maxBumpType(bumpedMembers.map((dir) => cumulativeBump(plan, dir, plan.bumps.get(dir)!.bumpType)))!
  const highestCurrent = group
    .map((dir) => participants.get(dir)!.currentVersion)
    .sort(compare)
    .at(-1)!
  const target = stableTarget(highestCurrent, groupBump)

  const laneTag = lanesByDir.get(group[0])
  if (laneTag == null) return target
  const nextN = Math.max(...group.map((dir) => nextPrereleaseNumber(participants.get(dir)!.currentVersion, target, laneTag)))
  return `${target}-${laneTag}.${nextN}`
}

/** Applies one round of bump propagation; returns whether any bump grew. */
function propagateBumps (plan: PlanState): boolean {
  const dependentsChanged = propagateToDependents(plan)
  const fixedGroupsChanged = propagateFixedGroupBumps(plan)
  const epicMembersChanged = seedEpicRebases(plan)
  return dependentsChanged || fixedGroupsChanged || epicMembersChanged
}

function propagateToDependents (plan: PlanState): boolean {
  let changed = false
  for (const dependent of plan.ctx.participants.values()) {
    for (const dep of dependent.internalDeps) {
      changed = propagateDependencyBump(plan, dependent, dep) || changed
    }
  }
  return changed
}

function propagateDependencyBump (plan: PlanState, dependent: Participant, dep: InternalDep): boolean {
  const target = plan.ctx.participants.get(dep.targetDir)
  const targetNewVersion = plan.newVersions.get(dep.targetDir)
  if (target == null || targetNewVersion == null) return false
  const materializedRange = materializeWorkspaceRange(dep.spec, target.currentVersion)
  if (materializedRange == null || satisfies(targetNewVersion, materializedRange)) return false
  const changed = bumpAtLeast(plan.bumps, { dir: dependent.dir, bumpType: 'patch', cause: 'dependencies' })
  plan.bumps.get(dependent.dir)!.dependencyUpdates.set(dep.targetName, targetNewVersion)
  return changed
}

function propagateFixedGroupBumps (plan: PlanState): boolean {
  let changed = false
  for (const group of plan.ctx.fixedGroups) {
    const groupBump = maxBumpType(group.map((dir) => plan.bumps.get(dir)?.bumpType))
    if (groupBump == null) continue
    for (const dir of group) {
      changed = bumpAtLeast(plan.bumps, { dir, bumpType: groupBump, cause: 'fixed' }) || changed
    }
  }
  return changed
}

/**
 * When the lead crosses to a new stable major, every member re-bases to
 * the band floor. Seed a release for each so the override in
 * applyEpicBandVersions has a version to replace and dependents propagate.
 */
function seedEpicRebases (plan: PlanState): boolean {
  let changed = false
  for (const epic of plan.ctx.epics) {
    if (epicRebaseFloor(epic, plan.ctx.participants, plan.newVersions) == null) continue
    for (const memberDir of epic.memberDirs) {
      changed = bumpAtLeast(plan.bumps, { dir: memberDir, bumpType: 'major', cause: 'epic' }) || changed
    }
  }
  return changed
}

function buildReleases (plan: PlanState): PlannedRelease[] {
  const releases = Array.from(plan.bumps.entries(), ([dir, pkgState]) => toPlannedRelease(plan, dir, pkgState))
  return releases.sort((left, right) => left.name.localeCompare(right.name) || left.dir.localeCompare(right.dir))
}

function toPlannedRelease (plan: PlanState, dir: string, pkgState: BumpState): PlannedRelease {
  const { participants, lanesByDir, opts } = plan.ctx
  const participant = participants.get(dir)!
  const consumedForChangelog = [
    ...(plan.pendingByDir.get(dir) ?? []),
    ...(lanesByDir.has(dir) ? [] : plan.laneConsumedByDir.get(dir) ?? []),
  ]
  return {
    name: participant.name,
    dir,
    rootDir: participant.rootDir,
    currentVersion: participant.currentVersion,
    newVersion: opts.snapshotSuffix != null ? `0.0.0-${opts.snapshotSuffix}` : plan.newVersions.get(dir)!,
    bumpType: pkgState.bumpType,
    intents: consumedForChangelog,
    dependencyUpdates: Array.from(pkgState.dependencyUpdates.entries())
      .map(([depName, newVersion]) => ({ name: depName, newVersion }))
      .sort((left, right) => left.name.localeCompare(right.name)),
    causes: Array.from(pkgState.causes).sort(),
  }
}

/**
 * A published `package@version` identifies exactly one artifact, so two
 * projects that share a name cannot both release the same version — the
 * registry would reject the second publish, and the name-keyed ledger entry
 * would collide. Caught here, before any manifest is written, naming both
 * directories.
 */
function assertNoDuplicateReleaseIdentity (releases: PlannedRelease[]): void {
  const byIdentity = new Map<string, string>()
  for (const release of releases) {
    const identity = `${release.name}@${release.newVersion}`
    const other = byIdentity.get(identity)
    if (other != null) {
      throw new PnpmError(
        'VERSIONING_DUPLICATE_RELEASE',
        `Two projects both release ${identity}: ./${other} and ./${release.dir}. ` +
        'A package name and version identify one published artifact, so same-named projects must release on different version lines (e.g. different lanes or majors).'
      )
    }
    byIdentity.set(identity, release.dir)
  }
}

function collectPendingIntents (ctx: AssembleContext): Map<string, ChangeIntent[]> {
  const pending = new Map<string, ChangeIntent[]>()
  for (const dir of ctx.participants.keys()) {
    const consumed = ctx.consumptionOf(dir)
    const pkgIntents = ctx.opts.intents.filter((intent) => {
      const bump = ctx.intentBumps.get(intent.id)?.get(dir)
      return bump != null && bump !== 'none' && !consumed.allIds.has(intent.id)
    })
    if (pkgIntents.length > 0) {
      pending.set(dir, pkgIntents)
    }
  }
  return pending
}

/**
 * Intents already consumed by prereleases of a package that has not graduated
 * to a stable version yet. They participate in the cumulative bump computation
 * of the package's lane and compose the stable changelog section at
 * graduation.
 */
function collectLaneConsumedIntents (ctx: AssembleContext): Map<string, ChangeIntent[]> {
  const laneConsumed = new Map<string, ChangeIntent[]>()
  for (const dir of ctx.participants.keys()) {
    const consumed = ctx.consumptionOf(dir)
    if (consumed.prereleaseOnlyIds.size === 0) continue
    const pkgIntents = ctx.opts.intents.filter((intent) => {
      const bump = ctx.intentBumps.get(intent.id)?.get(dir)
      return bump != null && bump !== 'none' && consumed.prereleaseOnlyIds.has(intent.id)
    })
    if (pkgIntents.length > 0) {
      laneConsumed.set(dir, pkgIntents)
    }
  }
  return laneConsumed
}

function enforceMaxBump (releases: PlannedRelease[], versioning?: VersioningSettings): void {
  const maxBump = versioning?.maxBump
  if (maxBump == null) return
  for (const release of releases) {
    const effectiveBump = effectiveBumpClass(release)
    if (BUMP_ORDER[effectiveBump] <= BUMP_ORDER[maxBump]) continue
    const intentFiles = release.intents
      .filter((intent) => Object.values(intent.releases).includes(effectiveBump))
      .map((intent) => intent.filePath)
    const raisedBy = intentFiles.length > 0 ? `intent file(s) ${intentFiles.join(', ')}` : `constraint chain: ${release.causes.join(', ')}`
    throw new PnpmError(
      'VERSIONING_MAX_BUMP_EXCEEDED',
      `The release plan bumps ${release.name} by ${effectiveBump}, but versioning.maxBump caps releases from this branch at ${maxBump}. Raised by ${raisedBy}.`
    )
  }
}

/**
 * The bump class a release actually applies. Fixed-group version sharing and
 * lane escalation can move a version further than the package's own
 * declared or propagated bump, so the cap compares against the real distance
 * between the current and the new version as well.
 */
function effectiveBumpClass (release: PlannedRelease): ReleaseBumpType {
  const diffClass = diff(release.currentVersion, release.newVersion)
  const normalized = diffClass?.replace(/^pre(?!release)/, '')
  return maxBumpType([release.bumpType, normalized ?? undefined]) ?? release.bumpType
}
