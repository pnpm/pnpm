import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import type { ProjectManifest, VersioningSettings } from '@pnpm/types'
import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'
import { compare, diff, inc, prerelease as parsePrerelease, satisfies, valid, validRange } from 'semver'

import type { ChangeIntent, IntentBumpType, ReleaseBumpType } from './intents.js'
import { buildConsumptionIndex, type Ledger, normalizeProjectDir, type PackageConsumption } from './ledger.js'

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
}

const BUMP_ORDER: Record<ReleaseBumpType, number> = { patch: 1, minor: 2, major: 3 }

const PROPAGATED_DEP_FIELDS = ['dependencies', 'optionalDependencies', 'peerDependencies'] as const

/**
 * Whether a package reference is a workspace-relative directory path rather
 * than a package name — the additive extension to the changesets format,
 * needed only when workspace projects share a published name.
 */
export function isDirRef (ref: string): boolean {
  return ref.startsWith('./')
}

/**
 * Resolves package references — bare names, or `./`-prefixed
 * workspace-relative directories — against the workspace. Names are aliases:
 * one that matches several projects cannot identify any of them and callers
 * must treat it as an error, never a silent pick.
 */
export interface ProjectRefIndex {
  /** The directories a reference resolves to: `[]` unknown, 2+ ambiguous. */
  refToDirs: (ref: string) => string[]
  nameToDirs: (name: string) => string[]
}

export function indexProjectRefs (
  projects: ReadonlyArray<{ rootDir: string, manifest: { name?: string } }>,
  workspaceDir: string
): ProjectRefIndex {
  const dirs = new Set<string>()
  const dirsByName = new Map<string, string[]>()
  for (const project of projects) {
    const dir = toProjectDir(workspaceDir, project.rootDir)
    dirs.add(dir)
    const name = project.manifest.name
    if (name == null) continue
    let named = dirsByName.get(name)
    if (named == null) {
      named = []
      dirsByName.set(name, named)
    }
    named.push(dir)
  }
  return {
    refToDirs: (ref) => {
      if (isDirRef(ref)) {
        const dir = normalizeProjectDir(ref)
        return dirs.has(dir) ? [dir] : []
      }
      return dirsByName.get(ref) ?? []
    },
    nameToDirs: (name) => dirsByName.get(name) ?? [],
  }
}

/** The workspace-relative directory of a project, in canonical spelling. */
export function toProjectDir (workspaceDir: string, rootDir: string): string {
  return normalizeProjectDir(path.relative(workspaceDir, rootDir))
}

export function assembleReleasePlan (opts: AssembleReleasePlanOptions): ReleasePlan {
  const refs = indexProjectRefs(opts.projects, opts.workspaceDir)
  const participants = collectParticipants(opts.projects, refs, opts)
  const lanesByDir = resolveLanes(refs, participants, opts.versioning)
  const fixedGroups = resolveFixedGroups(refs, participants, opts.versioning)
  validateFixedGroupLanes(fixedGroups, lanesByDir, opts.versioning)
  const intentBumps = resolveIntents(opts.intents, refs, participants)
  if (opts.enforceWorkspaceProtocol) {
    assertInternalDepsUseWorkspaceProtocol(participants)
  }
  const consumptionOf = buildConsumptionIndex(opts.ledger, refs.nameToDirs)

  const ctx: AssembleContext = { participants, lanesByDir, fixedGroups, intentBumps, consumptionOf, opts }
  let selection = opts.filter
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

interface Participant {
  name: string
  dir: string
  rootDir: string
  currentVersion: string
  manifest: ProjectManifest
  /** Workspace-internal production dependencies, by target project dir. */
  internalDeps: InternalDep[]
}

interface InternalDep {
  targetDir: string
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

interface AssembleContext {
  participants: Map<string, Participant>
  lanesByDir: Map<string, string>
  fixedGroups: string[][]
  /** Per intent id: the participant dirs it releases and their bump types. */
  intentBumps: Map<string, Map<string, IntentBumpType>>
  consumptionOf: (dir: string) => PackageConsumption
  opts: AssembleReleasePlanOptions
}

function assemble (ctx: AssembleContext, selection: Set<string> | undefined): ReleasePlan {
  const { participants, lanesByDir, fixedGroups, opts } = ctx
  const pendingByDir = collectPendingIntents(ctx)
  const laneConsumedByDir = collectLaneConsumedIntents(ctx)

  const state = new Map<string, BumpState>()
  const bumpAtLeast = (dir: string, bumpType: ReleaseBumpType, cause: ReleaseCause): boolean => {
    const existing = state.get(dir)
    if (existing == null) {
      state.set(dir, { bumpType, causes: new Set([cause]), dependencyUpdates: new Map() })
      return true
    }
    existing.causes.add(cause)
    if (BUMP_ORDER[bumpType] > BUMP_ORDER[existing.bumpType]) {
      existing.bumpType = bumpType
      return true
    }
    return false
  }
  const intentBumpFor = (intent: ChangeIntent, dir: string): IntentBumpType | undefined =>
    ctx.intentBumps.get(intent.id)?.get(dir)

  for (const [dir, pending] of pendingByDir.entries()) {
    if (selection != null && !selection.has(dir)) continue
    const direct = maxBumpType(pending.map((intent) => intentBumpFor(intent, dir)))
    if (direct != null) {
      bumpAtLeast(dir, direct, 'intent')
    }
  }

  // A package that left its lane releases the accumulated stable
  // version even when no new intents are pending.
  for (const [dir, laneConsumed] of laneConsumedByDir.entries()) {
    if (selection != null && !selection.has(dir)) continue
    if (lanesByDir.has(dir) || laneConsumed.length === 0) continue
    const graduated = maxBumpType(laneConsumed.map((intent) => intentBumpFor(intent, dir)))
    if (graduated != null) {
      bumpAtLeast(dir, graduated, 'intent')
    }
  }

  const cumulativeBump = (dir: string, planned: ReleaseBumpType): ReleaseBumpType => {
    const laneConsumed = laneConsumedByDir.get(dir) ?? []
    return maxBumpType([planned, ...laneConsumed.map((intent) => intentBumpFor(intent, dir))]) ?? planned
  }

  const newVersions = new Map<string, string>()
  const computeVersions = (): void => {
    newVersions.clear()
    for (const [dir, pkgState] of state.entries()) {
      const participant = participants.get(dir)!
      newVersions.set(dir, computeNewVersion(participant, pkgState.bumpType, {
        laneTag: lanesByDir.get(dir),
        cumulativeBump: cumulativeBump(dir, pkgState.bumpType),
      }))
    }
    applyFixedGroupVersions({ participants, state, newVersions, cumulativeBump, fixedGroups, lanesByDir })
  }

  for (let changed = true; changed;) {
    changed = false
    computeVersions()

    for (const dependent of participants.values()) {
      for (const dep of dependent.internalDeps) {
        const target = participants.get(dep.targetDir)
        const targetNewVersion = newVersions.get(dep.targetDir)
        if (target == null || targetNewVersion == null) continue
        const materializedRange = materializeWorkspaceRange(dep.spec, target.currentVersion)
        if (materializedRange == null || satisfies(targetNewVersion, materializedRange)) continue
        if (bumpAtLeast(dependent.dir, 'patch', 'dependencies')) {
          changed = true
        }
        state.get(dependent.dir)!.dependencyUpdates.set(dep.targetName, targetNewVersion)
      }
    }

    for (const group of fixedGroups) {
      const groupBump = maxBumpType(group.map((dir) => state.get(dir)?.bumpType))
      if (groupBump == null) continue
      for (const dir of group) {
        if (bumpAtLeast(dir, groupBump, 'fixed')) {
          changed = true
        }
      }
    }
  }
  computeVersions()

  const releases: PlannedRelease[] = []
  for (const [dir, pkgState] of state.entries()) {
    const participant = participants.get(dir)!
    const consumedForChangelog = [
      ...(pendingByDir.get(dir) ?? []),
      ...(lanesByDir.has(dir) ? [] : laneConsumedByDir.get(dir) ?? []),
    ]
    releases.push({
      name: participant.name,
      dir,
      rootDir: participant.rootDir,
      currentVersion: participant.currentVersion,
      newVersion: opts.snapshotSuffix != null ? `0.0.0-${opts.snapshotSuffix}` : newVersions.get(dir)!,
      bumpType: pkgState.bumpType,
      intents: consumedForChangelog,
      dependencyUpdates: Array.from(pkgState.dependencyUpdates.entries())
        .map(([depName, newVersion]) => ({ name: depName, newVersion }))
        .sort((left, right) => left.name.localeCompare(right.name)),
      causes: Array.from(pkgState.causes).sort(),
    })
  }
  releases.sort((left, right) => left.name.localeCompare(right.name) || left.dir.localeCompare(right.dir))

  assertNoDuplicateReleaseIdentity(releases)
  if (opts.snapshotSuffix == null) {
    enforceMaxBump(releases, opts.versioning)
  }

  return { releases }
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

function collectParticipants (
  projects: WorkspaceProject[],
  refs: ProjectRefIndex,
  opts: AssembleReleasePlanOptions
): Map<string, Participant> {
  const ignoredDirs = new Set<string>()
  for (const ref of opts.versioning?.ignore ?? []) {
    for (const dir of resolveConfigRef(refs, ref, 'versioning.ignore')) {
      ignoredDirs.add(dir)
    }
  }

  const participants = new Map<string, Participant>()
  for (const project of projects) {
    const { name, version } = project.manifest
    const dir = toProjectDir(opts.workspaceDir, project.rootDir)
    // What cannot release is excluded automatically: unnamed and versionless
    // (private) packages, packages with non-semver placeholder versions, and
    // the explicitly frozen ones.
    if (name == null || version == null || valid(version) == null || ignoredDirs.has(dir)) continue
    participants.set(dir, {
      name,
      dir,
      rootDir: project.rootDir,
      currentVersion: version,
      manifest: project.manifest,
      internalDeps: [],
    })
  }

  for (const participant of participants.values()) {
    for (const fieldName of PROPAGATED_DEP_FIELDS) {
      for (const [alias, spec] of Object.entries(participant.manifest[fieldName] ?? {})) {
        const targetName = internalDepTargetName(alias, spec, refs)
        if (targetName == null) continue
        const targetDirs = refs.nameToDirs(targetName).filter((dir) => participants.has(dir))
        if (targetDirs.length === 0) continue
        // A workspace: range naming an ambiguous package cannot be linked at
        // install time, so the release engine never legitimately sees one.
        if (targetDirs.length > 1) {
          throw new PnpmError(
            'VERSIONING_AMBIGUOUS_PACKAGE',
            `Package ${participant.name} (./${participant.dir}) depends on ${targetName}, which matches multiple workspace projects: ${targetDirs.map((dir) => `./${dir}`).join(', ')}`
          )
        }
        participant.internalDeps.push({ targetDir: targetDirs[0], targetName, fieldName, alias, spec })
      }
    }
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
function internalDepTargetName (alias: string, spec: string, refs: ProjectRefIndex): string | null {
  if (spec.startsWith('workspace:')) {
    const targetName = WorkspaceSpec.parse(spec)?.alias ?? alias
    return refs.nameToDirs(targetName).length > 0 ? targetName : null
  }
  if (refs.nameToDirs(alias).length === 0) return null
  if (spec.startsWith('catalog:') || validRange(spec) != null) return alias
  return null
}

/**
 * Resolves a package reference from `versioning` configuration. An unknown
 * reference is skipped — configuration may outlive a removed project — but an
 * ambiguous name is an error: it cannot be attributed, and silence here is
 * exactly the name-keying flaw this engine exists to fix.
 */
function resolveConfigRef (refs: ProjectRefIndex, ref: string, settingName: string): string[] {
  const dirs = refs.refToDirs(ref)
  if (dirs.length > 1) {
    throw new PnpmError(
      'VERSIONING_AMBIGUOUS_PACKAGE',
      `${settingName} references ${ref}, which matches multiple workspace projects: ${dirs.map((dir) => `./${dir}`).join(', ')}. Reference the project by directory instead.`
    )
  }
  return dirs
}

function resolveLanes (
  refs: ProjectRefIndex,
  participants: Map<string, Participant>,
  versioning?: VersioningSettings
): Map<string, string> {
  const lanesByDir = new Map<string, string>()
  for (const [ref, lane] of Object.entries(versioning?.lanes ?? {})) {
    if (lane.toLowerCase() === 'main') {
      throw new PnpmError(
        'VERSIONING_INVALID_LANE_NAME',
        `versioning.lanes assigns ${ref} to the "${lane}" lane, but "main" is the reserved default lane. Remove the entry instead.`
      )
    }
    for (const dir of resolveConfigRef(refs, ref, 'versioning.lanes')) {
      if (participants.has(dir)) {
        lanesByDir.set(dir, lane)
      }
    }
  }
  return lanesByDir
}

function resolveFixedGroups (
  refs: ProjectRefIndex,
  participants: Map<string, Participant>,
  versioning?: VersioningSettings
): string[][] {
  return (versioning?.fixed ?? []).map((group) =>
    group
      .flatMap((ref) => resolveConfigRef(refs, ref, 'versioning.fixed'))
      .filter((dir) => participants.has(dir)))
}

function validateFixedGroupLanes (
  fixedGroups: string[][],
  lanesByDir: Map<string, string>,
  versioning?: VersioningSettings
): void {
  for (const [index, group] of fixedGroups.entries()) {
    const tags = new Set(group.map((dir) => lanesByDir.get(dir)))
    if (tags.size > 1) {
      throw new PnpmError(
        'VERSIONING_CONFLICTING_CONFIG',
        `The fixed group [${(versioning?.fixed ?? [])[index].join(', ')}] mixes packages on different lanes. A fixed group must move between lanes together.`
      )
    }
  }
}

/**
 * Resolves every intent's package references to participant directories,
 * validating along the way: unknown references and names matching several
 * projects are hard errors, and a release can only be demanded from a
 * participant — otherwise the intent could never be consumed and the file
 * would linger forever. A `none` decline is fine for any workspace package.
 */
function resolveIntents (
  intents: ChangeIntent[],
  refs: ProjectRefIndex,
  participants: Map<string, Participant>
): Map<string, Map<string, IntentBumpType>> {
  const intentBumps = new Map<string, Map<string, IntentBumpType>>()
  for (const intent of intents) {
    const byDir = new Map<string, IntentBumpType>()
    for (const [ref, bumpType] of Object.entries(intent.releases)) {
      const dirs = refs.refToDirs(ref)
      if (dirs.length === 0) {
        throw new PnpmError('VERSIONING_UNKNOWN_PACKAGE', `Change intent file ${intent.filePath} names ${ref}, which is not a package in this workspace`)
      }
      if (dirs.length > 1) {
        throw new PnpmError(
          'VERSIONING_AMBIGUOUS_PACKAGE',
          `Change intent file ${intent.filePath} names ${ref}, which matches multiple workspace projects: ${dirs.map((dir) => `./${dir}`).join(', ')}. ` +
          'Reference the project by directory instead, e.g. "./' + dirs[0] + '": ' + bumpType
        )
      }
      const dir = dirs[0]
      if (bumpType !== 'none' && !participants.has(dir)) {
        throw new PnpmError(
          'VERSIONING_UNRELEASABLE_PACKAGE',
          `Change intent file ${intent.filePath} requests a ${bumpType} release of ${ref}, which cannot release ` +
          '(it is listed in versioning.ignore, has no version field, or has a non-semver version). ' +
          'Remove the entry or change it to "none".'
        )
      }
      const existing = byDir.get(dir)
      if (existing == null || (bumpType !== 'none' && BUMP_ORDER[bumpType] > (existing === 'none' ? 0 : BUMP_ORDER[existing]))) {
        byDir.set(dir, bumpType)
      }
    }
    intentBumps.set(intent.id, byDir)
  }
  return intentBumps
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
  cumulativeBump: (dir: string, planned: ReleaseBumpType) => ReleaseBumpType
  fixedGroups: string[][]
  lanesByDir: Map<string, string>
}

function applyFixedGroupVersions ({ participants, state, newVersions, cumulativeBump, fixedGroups, lanesByDir }: ApplyFixedGroupVersionsOptions): void {
  for (const group of fixedGroups) {
    const bumpedMembers = group.filter((dir) => state.has(dir))
    if (bumpedMembers.length === 0) continue

    const groupBump = maxBumpType(bumpedMembers.map((dir) => cumulativeBump(dir, state.get(dir)!.bumpType)))!
    const highestCurrent = group
      .map((dir) => participants.get(dir)!.currentVersion)
      .sort(compare)
      .at(-1)!
    const target = parsePrerelease(highestCurrent) == null
      ? inc(highestCurrent, groupBump)!
      : escalateStableTarget(stablePart(highestCurrent), groupBump)

    const laneTag = lanesByDir.get(group[0])
    let sharedVersion = target
    if (laneTag != null) {
      const nextN = Math.max(...group.map((dir) => nextPrereleaseNumber(participants.get(dir)!.currentVersion, target, laneTag)))
      sharedVersion = `${target}-${laneTag}.${nextN}`
    }
    for (const dir of group) {
      if (state.has(dir)) {
        newVersions.set(dir, sharedVersion)
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
