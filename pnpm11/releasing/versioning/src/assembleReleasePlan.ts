import { PnpmError } from '@pnpm/error'
import type { ProjectManifest, VersioningSettings } from '@pnpm/types'
import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'
import { compare, diff, inc, prerelease as parsePrerelease, satisfies, valid, validRange } from 'semver'

import type { ChangeIntent, ReleaseBumpType } from './intents.js'
import { buildConsumptionIndex, type Ledger, type PackageConsumption } from './ledger.js'

export interface WorkspaceProject {
  rootDir: string
  manifest: ProjectManifest
}

export type ReleaseCause = 'intent' | 'dependencies' | 'fixed'

export interface DependencyUpdate {
  name: string
  newVersion: string
}

export interface PlannedRelease {
  name: string
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
  projects: WorkspaceProject[]
  intents: ChangeIntent[]
  ledger: Ledger
  versioning?: VersioningSettings
  /**
   * Package names selected with --filter. The plan is narrowed to the selected
   * packages' portion of the pending work, expanded with their fixed-group
   * companions and range-invalidated dependents.
   */
  filter?: Set<string>
  /**
   * When set, every planned release gets the version `0.0.0-<suffix>` instead
   * of the computed one, matching snapshot releases.
   */
  snapshotSuffix?: string
}

const BUMP_ORDER: Record<ReleaseBumpType, number> = { patch: 1, minor: 2, major: 3 }

const PROPAGATED_DEP_FIELDS = ['dependencies', 'optionalDependencies', 'peerDependencies'] as const

export function assembleReleasePlan (opts: AssembleReleasePlanOptions): ReleasePlan {
  const participants = collectParticipants(opts.projects, opts.versioning)
  validateVersioningConfig(participants, opts.versioning)
  validateIntents(opts.intents, opts.projects, participants)
  assertInternalDepsUseWorkspaceProtocol(participants)
  const consumptionOf = buildConsumptionIndex(opts.ledger)

  let selection = opts.filter
  for (;;) {
    const plan = assemble(participants, consumptionOf, selection, opts)
    if (selection == null) return plan
    const expanded = new Set(selection)
    for (const release of plan.releases) {
      expanded.add(release.name)
    }
    if (expanded.size === selection.size) return plan
    selection = expanded
  }
}

interface Participant {
  name: string
  rootDir: string
  currentVersion: string
  manifest: ProjectManifest
  /** Workspace-internal production dependencies, by target package name. */
  internalDeps: InternalDep[]
}

interface InternalDep {
  targetName: string
  fieldName: typeof PROPAGATED_DEP_FIELDS[number]
  alias: string
  spec: string
}

interface BumpState {
  bumpType: ReleaseBumpType
  causes: Set<ReleaseCause>
  dependencyUpdates: Map<string, string>
}

function assemble (
  participants: Map<string, Participant>,
  consumptionOf: (pkgName: string) => PackageConsumption,
  selection: Set<string> | undefined,
  opts: AssembleReleasePlanOptions
): ReleasePlan {
  const pendingByPkg = collectPendingIntents(participants, opts.intents, consumptionOf)
  const laneConsumedByPkg = collectLaneConsumedIntents(participants, opts.intents, consumptionOf)
  const lanes = opts.versioning?.lanes ?? {}

  const state = new Map<string, BumpState>()
  const bumpAtLeast = (name: string, bumpType: ReleaseBumpType, cause: ReleaseCause): boolean => {
    const existing = state.get(name)
    if (existing == null) {
      state.set(name, { bumpType, causes: new Set([cause]), dependencyUpdates: new Map() })
      return true
    }
    existing.causes.add(cause)
    if (BUMP_ORDER[bumpType] > BUMP_ORDER[existing.bumpType]) {
      existing.bumpType = bumpType
      return true
    }
    return false
  }

  for (const [name, pending] of pendingByPkg.entries()) {
    if (selection != null && !selection.has(name)) continue
    const direct = maxBumpType(pending.map((intent) => intent.releases[name]))
    if (direct != null) {
      bumpAtLeast(name, direct, 'intent')
    }
  }

  // A package that left its lane releases the accumulated stable
  // version even when no new intents are pending.
  for (const [name, laneConsumed] of laneConsumedByPkg.entries()) {
    if (selection != null && !selection.has(name)) continue
    if (lanes[name] != null || laneConsumed.length === 0) continue
    const graduated = maxBumpType(laneConsumed.map((intent) => intent.releases[name]))
    if (graduated != null) {
      bumpAtLeast(name, graduated, 'intent')
    }
  }

  const newVersions = new Map<string, string>()
  const computeVersions = (): void => {
    newVersions.clear()
    for (const [name, pkgState] of state.entries()) {
      const participant = participants.get(name)!
      newVersions.set(name, computeNewVersion(participant, pkgState.bumpType, {
        laneTag: lanes[name],
        cumulativeBump: cumulativeBumpType(name, pkgState.bumpType, laneConsumedByPkg),
      }))
    }
    applyFixedGroupVersions({ participants, state, newVersions, laneConsumedByPkg, versioning: opts.versioning })
  }

  for (let changed = true; changed;) {
    changed = false
    computeVersions()

    for (const dependent of participants.values()) {
      for (const dep of dependent.internalDeps) {
        const target = participants.get(dep.targetName)
        const targetNewVersion = newVersions.get(dep.targetName)
        if (target == null || targetNewVersion == null) continue
        const materializedRange = materializeWorkspaceRange(dep.spec, target.currentVersion)
        if (materializedRange == null || satisfies(targetNewVersion, materializedRange)) continue
        if (bumpAtLeast(dependent.name, 'patch', 'dependencies')) {
          changed = true
        }
        state.get(dependent.name)!.dependencyUpdates.set(dep.targetName, targetNewVersion)
      }
    }

    for (const group of opts.versioning?.fixed ?? []) {
      const members = group.filter((name) => participants.has(name))
      const groupBump = maxBumpType(members.map((name) => state.get(name)?.bumpType))
      if (groupBump == null) continue
      for (const name of members) {
        if (bumpAtLeast(name, groupBump, 'fixed')) {
          changed = true
        }
      }
    }
  }
  computeVersions()

  const releases: PlannedRelease[] = []
  for (const [name, pkgState] of state.entries()) {
    const participant = participants.get(name)!
    const consumedForChangelog = [
      ...(pendingByPkg.get(name) ?? []),
      ...(lanes[name] == null ? laneConsumedByPkg.get(name) ?? [] : []),
    ]
    releases.push({
      name,
      rootDir: participant.rootDir,
      currentVersion: participant.currentVersion,
      newVersion: opts.snapshotSuffix != null ? `0.0.0-${opts.snapshotSuffix}` : newVersions.get(name)!,
      bumpType: pkgState.bumpType,
      intents: consumedForChangelog,
      dependencyUpdates: Array.from(pkgState.dependencyUpdates.entries())
        .map(([depName, newVersion]) => ({ name: depName, newVersion }))
        .sort((left, right) => left.name.localeCompare(right.name)),
      causes: Array.from(pkgState.causes).sort(),
    })
  }
  releases.sort((left, right) => left.name.localeCompare(right.name))

  if (opts.snapshotSuffix == null) {
    enforceMaxBump(releases, opts.versioning)
  }

  return { releases }
}

function collectParticipants (projects: WorkspaceProject[], versioning?: VersioningSettings): Map<string, Participant> {
  const ignored = new Set(versioning?.ignore ?? [])
  const names = new Set<string>()
  for (const project of projects) {
    if (project.manifest.name != null) {
      names.add(project.manifest.name)
    }
  }

  const participants = new Map<string, Participant>()
  for (const project of projects) {
    const { name, version } = project.manifest
    // What cannot release is excluded automatically: unnamed and versionless
    // (private) packages, packages with non-semver placeholder versions, and
    // the explicitly frozen ones.
    if (name == null || version == null || valid(version) == null || ignored.has(name)) continue
    const internalDeps: InternalDep[] = []
    for (const fieldName of PROPAGATED_DEP_FIELDS) {
      for (const [alias, spec] of Object.entries(project.manifest[fieldName] ?? {})) {
        const targetName = resolveInternalDepTarget(alias, spec, names)
        if (targetName == null || ignored.has(targetName)) continue
        internalDeps.push({ targetName, fieldName, alias, spec })
      }
    }
    participants.set(name, {
      name,
      rootDir: project.rootDir,
      currentVersion: version,
      manifest: project.manifest,
      internalDeps,
    })
  }
  return participants
}

/**
 * Decides whether a dependency entry points at a workspace package. Aliased
 * specs targeting somewhere else (`npm:`, `file:`, git URLs, …) are external
 * even when the alias collides with a workspace package name; a plain semver
 * range or `catalog:` entry on a workspace name is internal — it is exactly
 * the declaration the workspace-protocol check must reject.
 */
function resolveInternalDepTarget (alias: string, spec: string, workspaceNames: Set<string>): string | null {
  if (spec.startsWith('workspace:')) {
    const targetName = WorkspaceSpec.parse(spec)?.alias ?? alias
    return workspaceNames.has(targetName) ? targetName : null
  }
  if (!workspaceNames.has(alias)) return null
  if (spec.startsWith('catalog:') || validRange(spec) != null) return alias
  return null
}

function validateVersioningConfig (participants: Map<string, Participant>, versioning?: VersioningSettings): void {
  if (versioning == null) return
  const lanes = versioning.lanes ?? {}
  for (const group of versioning.fixed ?? []) {
    const members = group.filter((name) => participants.has(name))
    const tags = new Set(members.map((name) => lanes[name]))
    if (tags.size > 1) {
      throw new PnpmError(
        'VERSIONING_CONFLICTING_CONFIG',
        `The fixed group [${group.join(', ')}] mixes packages on different lanes. A fixed group must move between lanes together.`
      )
    }
  }
}

function validateIntents (intents: ChangeIntent[], projects: WorkspaceProject[], participants: Map<string, Participant>): void {
  const workspaceNames = new Set<string>()
  for (const project of projects) {
    if (project.manifest.name != null) {
      workspaceNames.add(project.manifest.name)
    }
  }
  for (const intent of intents) {
    for (const [pkgName, bumpType] of Object.entries(intent.releases)) {
      if (!workspaceNames.has(pkgName)) {
        throw new PnpmError('VERSIONING_UNKNOWN_PACKAGE', `Change intent file ${intent.filePath} names ${pkgName}, which is not a package in this workspace`)
      }
      // A "none" decline is fine for any workspace package, but a release can
      // only be demanded from a participant — otherwise the intent could never
      // be consumed and the file would linger forever.
      if (bumpType !== 'none' && !participants.has(pkgName)) {
        throw new PnpmError(
          'VERSIONING_UNRELEASABLE_PACKAGE',
          `Change intent file ${intent.filePath} requests a ${bumpType} release of ${pkgName}, which cannot release ` +
          '(it is listed in versioning.ignore, has no version field, or has a non-semver version). ' +
          'Remove the entry or change it to "none".'
        )
      }
    }
  }
}

function assertInternalDepsUseWorkspaceProtocol (participants: Map<string, Participant>): void {
  for (const participant of participants.values()) {
    for (const dep of participant.internalDeps) {
      if (!dep.spec.startsWith('workspace:')) {
        throw new PnpmError(
          'VERSIONING_INTERNAL_RANGE',
          `Package ${participant.name} declares the internal dependency ${dep.alias} in ${dep.fieldName} as "${dep.spec}". ` +
          'Internal dependencies must use the workspace: protocol so that dependency ranges never need rewriting at release time.'
        )
      }
    }
  }
}

function collectPendingIntents (
  participants: Map<string, Participant>,
  intents: ChangeIntent[],
  consumptionOf: (pkgName: string) => PackageConsumption
): Map<string, ChangeIntent[]> {
  const pending = new Map<string, ChangeIntent[]>()
  for (const name of participants.keys()) {
    const consumed = consumptionOf(name)
    const pkgIntents = intents.filter((intent) =>
      intent.releases[name] != null &&
      intent.releases[name] !== 'none' &&
      !consumed.allIds.has(intent.id))
    if (pkgIntents.length > 0) {
      pending.set(name, pkgIntents)
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
function collectLaneConsumedIntents (
  participants: Map<string, Participant>,
  intents: ChangeIntent[],
  consumptionOf: (pkgName: string) => PackageConsumption
): Map<string, ChangeIntent[]> {
  const laneConsumed = new Map<string, ChangeIntent[]>()
  for (const name of participants.keys()) {
    const consumed = consumptionOf(name)
    if (consumed.prereleaseOnlyIds.size === 0) continue
    const pkgIntents = intents.filter((intent) =>
      intent.releases[name] != null &&
      intent.releases[name] !== 'none' &&
      consumed.prereleaseOnlyIds.has(intent.id))
    if (pkgIntents.length > 0) {
      laneConsumed.set(name, pkgIntents)
    }
  }
  return laneConsumed
}

function cumulativeBumpType (name: string, planned: ReleaseBumpType, laneConsumedByPkg: Map<string, ChangeIntent[]>): ReleaseBumpType {
  const laneConsumed = laneConsumedByPkg.get(name) ?? []
  return maxBumpType([planned, ...laneConsumed.map((intent) => intent.releases[name])]) ?? planned
}

function maxBumpType (types: Array<string | undefined>): ReleaseBumpType | null {
  let result: ReleaseBumpType | null = null
  for (const type of types) {
    if (type !== 'patch' && type !== 'minor' && type !== 'major') continue
    if (result == null || BUMP_ORDER[type] > BUMP_ORDER[result]) {
      result = type
    }
  }
  return result
}

interface NewVersionOptions {
  laneTag?: string
  /**
   * The highest bump accumulated across the package's lane — the
   * planned bump joined with the bumps of intents consumed by earlier
   * prereleases — which keeps the stable target stable across `-tag.N` runs.
   */
  cumulativeBump: ReleaseBumpType
}

function computeNewVersion (participant: Participant, bumpType: ReleaseBumpType, opts: NewVersionOptions): string {
  const current = participant.currentVersion
  if (opts.laneTag == null) {
    if (parsePrerelease(current) == null) {
      return inc(current, bumpType)!
    }
    // Graduation: the accumulated stable version the lane was
    // building toward.
    return escalateStableTarget(stablePart(current), opts.cumulativeBump)
  }
  const target = parsePrerelease(current) == null
    ? inc(current, opts.cumulativeBump)!
    : escalateStableTarget(stablePart(current), opts.cumulativeBump)
  return `${target}-${opts.laneTag}.${nextPrereleaseNumber(current, target, opts.laneTag)}`
}

/**
 * Re-derives the stable version a lane is building toward when the
 * cumulative bump escalates. The invariant: the stable part of the current
 * prerelease already reflects the previous cumulative bump applied to the
 * version the line started from, so only an escalation changes it.
 */
function escalateStableTarget (target: string, cumulativeBump: ReleaseBumpType): string {
  const [major, minor, patch] = target.split('.').map(Number)
  switch (cumulativeBump) {
    case 'major':
      return minor === 0 && patch === 0 ? target : `${major + 1}.0.0`
    case 'minor':
      return patch === 0 ? target : `${major}.${minor + 1}.0`
    case 'patch':
      return target
  }
}

function stablePart (version: string): string {
  return version.split('-')[0]
}

function nextPrereleaseNumber (current: string, target: string, laneTag: string): number {
  const currentPrerelease = parsePrerelease(current)
  if (currentPrerelease == null) return 0
  const [currentTag, currentN] = currentPrerelease
  // semver parses an all-digit prerelease identifier as a number, so the tag
  // comparison must not be strict about the type.
  if (stablePart(current) !== target || String(currentTag) !== laneTag || typeof currentN !== 'number') return 0
  return currentN + 1
}

interface ApplyFixedGroupVersionsOptions {
  participants: Map<string, Participant>
  state: Map<string, BumpState>
  newVersions: Map<string, string>
  laneConsumedByPkg: Map<string, ChangeIntent[]>
  versioning?: VersioningSettings
}

function applyFixedGroupVersions ({ participants, state, newVersions, laneConsumedByPkg, versioning }: ApplyFixedGroupVersionsOptions): void {
  const lanes = versioning?.lanes ?? {}
  for (const group of versioning?.fixed ?? []) {
    const members = group.filter((name) => participants.has(name))
    const bumpedMembers = members.filter((name) => state.has(name))
    if (bumpedMembers.length === 0) continue

    const groupBump = maxBumpType(bumpedMembers.map((name) => cumulativeBumpType(name, state.get(name)!.bumpType, laneConsumedByPkg)))!
    const highestCurrent = members
      .map((name) => participants.get(name)!.currentVersion)
      .sort(compare)
      .at(-1)!
    const target = parsePrerelease(highestCurrent) == null
      ? inc(highestCurrent, groupBump)!
      : escalateStableTarget(stablePart(highestCurrent), groupBump)

    const laneTag = lanes[members[0]]
    let sharedVersion = target
    if (laneTag != null) {
      const nextN = Math.max(...members.map((name) => nextPrereleaseNumber(participants.get(name)!.currentVersion, target, laneTag)))
      sharedVersion = `${target}-${laneTag}.${nextN}`
    }
    for (const name of members) {
      if (state.has(name)) {
        newVersions.set(name, sharedVersion)
      }
    }
  }
}

/**
 * The range that pnpm materializes for a workspace: spec at pack time, given
 * the dependency's version at the dependent's previous release. Dependent
 * propagation republishes the dependent whenever the dependency's new version
 * falls outside this range.
 */
export function materializeWorkspaceRange (spec: string, depCurrentVersion: string): string | null {
  const parsed = WorkspaceSpec.parse(spec)
  if (parsed == null) return null
  switch (parsed.version) {
    case '^':
      return `^${depCurrentVersion}`
    case '~':
      return `~${depCurrentVersion}`
    case '*':
    case '':
      return depCurrentVersion
    default:
      return parsed.version
  }
}

function enforceMaxBump (releases: PlannedRelease[], versioning?: VersioningSettings): void {
  const maxBump = versioning?.maxBump
  if (maxBump == null) return
  for (const release of releases) {
    const effectiveBump = effectiveBumpClass(release)
    if (BUMP_ORDER[effectiveBump] <= BUMP_ORDER[maxBump]) continue
    const intentFiles = release.intents
      .filter((intent) => intent.releases[release.name] === effectiveBump)
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
