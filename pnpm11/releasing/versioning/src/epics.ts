import { PnpmError } from '@pnpm/error'
import type { VersioningSettings } from '@pnpm/types'
import { prerelease as parsePrerelease } from 'semver'

import { normalizeProjectDir } from './ledger.js'
import type { Participant } from './participants.js'
import { isDirRef, type ProjectRefIndex, resolveConfigRef } from './projectRefs.js'
import { nextPrereleaseNumber, stablePart } from './versionMath.js'

/**
 * An epic resolved against the workspace: the lead's directory and the
 * directories of its member packages. The lead is never a member of its own
 * band.
 */
export interface ResolvedEpic {
  leadRef: string
  leadDir: string
  memberDirs: Set<string>
}

type EpicConfig = NonNullable<VersioningSettings['epics']>[number]

/**
 * Resolves each configured epic to its lead directory and the set of member
 * directories its selectors match. The lead — a single named package with a
 * semver version — is excluded from its own membership; a selector matching
 * it is a no-op. Membership selectors match name globs, `./`-prefixed
 * directory globs, and `!`-prefixed negations.
 */
export function resolveEpics (
  refs: ProjectRefIndex,
  participants: Map<string, Participant>,
  versioning?: VersioningSettings
): ResolvedEpic[] {
  return (versioning?.epics ?? []).map((epic) => {
    const leadDir = resolveEpicLeadDir(refs, participants, epic.lead)
    return { leadRef: epic.lead, leadDir, memberDirs: collectEpicMemberDirs(epic, leadDir, participants) }
  })
}

function resolveEpicLeadDir (refs: ProjectRefIndex, participants: Map<string, Participant>, lead: string): string {
  const leadDir = resolveConfigRef(refs, lead, 'versioning.epics lead')[0]
  if (leadDir == null || !participants.has(leadDir)) {
    throw new PnpmError(
      'VERSIONING_EPIC_UNKNOWN_LEAD',
      `versioning.epics lead "${lead}" is not a releasable workspace project (it must be a named package with a semver version).`
    )
  }
  return leadDir
}

function collectEpicMemberDirs (epic: EpicConfig, leadDir: string, participants: Map<string, Participant>): Set<string> {
  const selectors = epic.packages.map(compileEpicSelector)
  const memberDirs = new Set<string>()
  for (const participant of participants.values()) {
    if (participant.dir === leadDir) continue
    if (matchesEpicSelectors(selectors, participant.dir, participant.name)) {
      memberDirs.add(participant.dir)
    }
  }
  return memberDirs
}

interface EpicSelector {
  negated: boolean
  /** Whether the pattern matches a project's directory rather than its name. */
  onDir: boolean
  match: (input: string) => boolean
}

function compileEpicSelector (selector: string): EpicSelector {
  const negated = selector.startsWith('!')
  const body = negated ? selector.slice(1) : selector
  const onDir = isDirRef(body)
  return { negated, onDir, match: wildcardMatch(onDir ? normalizeProjectDir(body) : body) }
}

/**
 * Whether a project is an epic member under pnpm's order-dependent selector
 * rule: each matching selector overrides the previous verdict, so the last one
 * to match decides — a positive include or a `!` negation — mirroring
 * `@pnpm/config.matcher`, where a later include can re-include a package an
 * earlier negation excluded.
 */
function matchesEpicSelectors (selectors: EpicSelector[], dir: string, name: string): boolean {
  let included = false
  for (const selector of selectors) {
    if (selector.match(selector.onDir ? dir : name)) {
      included = !selector.negated
    }
  }
  return included
}

/**
 * Compiles a selector where `*` matches any run of characters and every other
 * character is literal, mirroring `@pnpm/config.matcher`'s wildcard semantics
 * so epic membership globs behave like pnpm's other package selectors.
 */
function wildcardMatch (pattern: string): (input: string) => boolean {
  if (pattern === '*') return () => true
  let source = '^'
  for (const character of pattern) {
    source += character === '*' ? '.*' : character.replace(/[.+?^${}()|[\]\\]/g, '\\$&')
  }
  source += '$'
  const regexp = new RegExp(source)
  return (input) => regexp.test(input)
}

/**
 * Rejects epic configurations that cannot be attributed unambiguously: a
 * package matched by two epics, and a fixed group that straddles an epic
 * boundary (a group must sit entirely inside or entirely outside an epic, so
 * its members never disagree on whether they are band-constrained).
 */
export function validateEpics (epics: ResolvedEpic[], fixedGroups: string[][]): void {
  assertNoEpicOverlap(epics)
  for (const epic of epics) {
    for (const group of fixedGroups) {
      assertFixedGroupDoesNotStraddleEpic(epic, group)
    }
  }
}

function assertNoEpicOverlap (epics: ResolvedEpic[]): void {
  const epicOfMember = new Map<string, string>()
  for (const epic of epics) {
    for (const memberDir of epic.memberDirs) {
      const other = epicOfMember.get(memberDir)
      if (other != null && other !== epic.leadRef) {
        throw new PnpmError(
          'VERSIONING_EPIC_OVERLAP',
          `Package ./${memberDir} is matched by two epics (leads "${other}" and "${epic.leadRef}"). A package can belong to at most one epic.`
        )
      }
      epicOfMember.set(memberDir, epic.leadRef)
    }
  }
}

function assertFixedGroupDoesNotStraddleEpic (epic: ResolvedEpic, group: string[]): void {
  if (!group.some((dir) => epic.memberDirs.has(dir))) return
  const outsiders = group.filter((dir) => !epic.memberDirs.has(dir))
  if (outsiders.length > 0) {
    throw new PnpmError(
      'VERSIONING_EPIC_FIXED_GROUP_CONFLICT',
      `A fixed group straddles the epic led by "${epic.leadRef}": it mixes epic members with outside package(s) ${outsiders.map((dir) => `./${dir}`).join(', ')}. A fixed group must sit entirely inside or entirely outside an epic.`
    )
  }
}

/**
 * The band floor (`newMajor × 100`) an epic re-bases its members to, or null
 * when no re-base is due. A re-base fires only when the lead releases to a
 * new, higher *stable* major in this plan; a prerelease lead version (the lead
 * on a lane) defers the re-base until its stable release.
 */
export function epicRebaseFloor (
  epic: ResolvedEpic,
  participants: Map<string, Participant>,
  newVersions: Map<string, string>
): number | null {
  const lead = participants.get(epic.leadDir)
  const newLeadVersion = newVersions.get(epic.leadDir)
  if (lead == null || newLeadVersion == null || parsePrerelease(newLeadVersion) != null) return null
  const newMajor = Number(newLeadVersion.split('.')[0])
  const currentMajor = epicLeadBandMajor(lead.currentVersion)
  return newMajor > currentMajor ? newMajor * 100 : null
}

function epicLeadBandMajor (version: string): number {
  const [major, minor, patch] = stablePart(version).split('.').map(Number)
  return parsePrerelease(version) != null && minor === 0 && patch === 0 ? Math.max(0, major - 1) : major
}

export interface ApplyEpicBandVersionsOptions {
  participants: Map<string, Participant>
  /** The dirs the plan releases. */
  state: ReadonlyMap<string, unknown>
  newVersions: Map<string, string>
  epics: ResolvedEpic[]
  lanesByDir: Map<string, string>
}

/**
 * Overrides the computed version of every bumped epic member with the band
 * floor when its lead crosses to a new stable major. A member on a lane
 * re-bases to a prerelease of the floor; every other member to `floor.0.0`.
 */
export function applyEpicBandVersions (opts: ApplyEpicBandVersionsOptions): void {
  const { participants, state, newVersions, epics } = opts
  for (const epic of epics) {
    const floor = epicRebaseFloor(epic, participants, newVersions)
    if (floor == null) continue
    const target = `${floor}.0.0`
    for (const memberDir of epic.memberDirs) {
      if (!state.has(memberDir)) continue
      newVersions.set(memberDir, rebasedMemberVersion(opts, memberDir, target))
    }
  }
}

function rebasedMemberVersion (opts: ApplyEpicBandVersionsOptions, memberDir: string, target: string): string {
  const laneTag = opts.lanesByDir.get(memberDir)
  if (laneTag == null) return target
  return `${target}-${laneTag}.${nextPrereleaseNumber(opts.participants.get(memberDir)!.currentVersion, target, laneTag)}`
}

/**
 * The band of member majors an epic permits: `[leadMajor×100, leadMajor×100+99]`,
 * where `leadMajor` is the major the plan establishes for the lead — its
 * re-based major when the lead crosses to a new stable major, otherwise the
 * lead's current major (a prerelease lead does not open the next band).
 */
export interface EpicBand {
  major: number
  low: number
  high: number
  contains: (memberMajor: number) => boolean
}

export function epicBand (
  epic: ResolvedEpic,
  participants: Map<string, Participant>,
  newVersions: Map<string, string>
): EpicBand {
  const floor = epicRebaseFloor(epic, participants, newVersions)
  const major = floor != null ? floor / 100 : epicLeadBandMajor(participants.get(epic.leadDir)!.currentVersion)
  const low = major * 100
  const high = low + 99
  return { major, low, high, contains: (memberMajor) => memberMajor >= low && memberMajor <= high }
}

/**
 * Enforces that every released member's new major stays inside its epic's band.
 * The re-base already keeps members in band when the lead moves; this guards
 * the other direction — an ordinary `major` intent that would carry a member
 * over the band ceiling (`1199.x` → `1200.0.0` while the lead is still on 11)
 * is rejected rather than silently landing the member in the next band.
 */
export function enforceEpicBands (
  epics: ResolvedEpic[],
  participants: Map<string, Participant>,
  newVersions: Map<string, string>
): void {
  for (const epic of epics) {
    const band = epicBand(epic, participants, newVersions)
    for (const memberDir of epic.memberDirs) {
      const memberVersion = newVersions.get(memberDir)
      if (memberVersion == null) continue
      assertMemberInBand({ epic, band, memberName: participants.get(memberDir)!.name, memberVersion })
    }
  }
}

interface MemberInBandCheck {
  epic: ResolvedEpic
  band: EpicBand
  memberName: string
  memberVersion: string
}

function assertMemberInBand ({ epic, band, memberName, memberVersion }: MemberInBandCheck): void {
  const memberMajor = Number(memberVersion.split('.')[0])
  if (band.contains(memberMajor)) return
  throw new PnpmError(
    'VERSIONING_EPIC_OUT_OF_BAND',
    `The release plan takes ${memberName} to ${memberVersion}, whose major ${memberMajor} is outside the band ${band.low}-${band.high} of the epic led by "${epic.leadRef}" (major ${band.major}). ` +
    (memberMajor > band.high
      ? 'The band is exhausted - the lead must advance to a new major to open the next band.'
      : 'Re-base the member into the band, or remove it from the epic.')
  )
}
