import { PnpmError } from '@pnpm/error'
import type { VersioningSettings } from '@pnpm/types'

import { type ResolvedEpic, resolveEpics, validateEpics } from './epics.js'
import type { ChangeIntent, IntentBumpType } from './intents.js'
import { collectParticipants, type Participant } from './participants.js'
import { indexProjectRefs, type ProjectRefIndex, resolveConfigRef, type WorkspaceProject } from './projectRefs.js'
import { BUMP_ORDER } from './versionMath.js'

export interface ResolveWorkspaceVersioningOptions {
  workspaceDir: string
  projects: WorkspaceProject[]
  versioning?: VersioningSettings
}

/** The workspace's `versioning` configuration, resolved against its projects and validated. */
export interface WorkspaceVersioning {
  refs: ProjectRefIndex
  participants: Map<string, Participant>
  lanesByDir: Map<string, string>
  fixedGroups: string[][]
  epics: ResolvedEpic[]
}

export function resolveWorkspaceVersioning (opts: ResolveWorkspaceVersioningOptions): WorkspaceVersioning {
  const refs = indexProjectRefs(opts.projects, opts.workspaceDir)
  const participants = collectParticipants(opts.projects, refs, opts)
  const lanesByDir = resolveLanes(refs, participants, opts.versioning)
  const fixedGroups = resolveFixedGroups(refs, participants, opts.versioning)
  validateFixedGroupLanes(fixedGroups, lanesByDir, opts.versioning)
  const epics = resolveEpics(refs, participants, opts.versioning)
  validateEpics(epics, fixedGroups)
  return { refs, participants, lanesByDir, fixedGroups, epics }
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

interface IntentRefScope {
  refs: ProjectRefIndex
  participants: Map<string, Participant>
}

/**
 * Resolves every intent's package references to participant directories,
 * validating along the way: unknown references and names matching several
 * projects are hard errors, and a release can only be demanded from a
 * participant — otherwise the intent could never be consumed and the file
 * would linger forever. A `none` decline is fine for any workspace package.
 */
export function resolveIntents (
  intents: ChangeIntent[],
  refs: ProjectRefIndex,
  participants: Map<string, Participant>
): Map<string, Map<string, IntentBumpType>> {
  const intentBumps = new Map<string, Map<string, IntentBumpType>>()
  for (const intent of intents) {
    intentBumps.set(intent.id, resolveIntentBumps(intent, { refs, participants }))
  }
  return intentBumps
}

function resolveIntentBumps (intent: ChangeIntent, scope: IntentRefScope): Map<string, IntentBumpType> {
  const byDir = new Map<string, IntentBumpType>()
  for (const [ref, bumpType] of Object.entries(intent.releases)) {
    const dir = resolveIntentRef(intent, [ref, bumpType], scope)
    if (outranksBump(bumpType, byDir.get(dir))) {
      byDir.set(dir, bumpType)
    }
  }
  return byDir
}

function resolveIntentRef (
  intent: ChangeIntent,
  [ref, bumpType]: [string, IntentBumpType],
  { refs, participants }: IntentRefScope
): string {
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
  return dir
}

function outranksBump (bumpType: IntentBumpType, existing: IntentBumpType | undefined): boolean {
  if (existing == null) return true
  if (bumpType === 'none') return false
  return BUMP_ORDER[bumpType] > (existing === 'none' ? 0 : BUMP_ORDER[existing])
}
