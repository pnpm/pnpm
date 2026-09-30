import type { VersioningSettings } from '@pnpm/types'

import { epicBand, type ResolvedEpic } from './epics.js'
import type { Participant } from './participants.js'
import type { WorkspaceProject } from './projectRefs.js'
import { resolveWorkspaceVersioning } from './workspaceVersioning.js'

export interface VersioningInvariantViolation {
  code: 'VERSIONING_EPIC_OUT_OF_BAND' | 'VERSIONING_FIXED_GROUP_MISMATCH'
  message: string
}

export interface CheckVersioningInvariantsOptions {
  workspaceDir: string
  projects: WorkspaceProject[]
  versioning?: VersioningSettings
}

/**
 * Validates that the committed versions already satisfy the invariants the
 * configuration declares: every epic member's major sits inside its lead's
 * band, and every fixed group shares one version. This is the static
 * counterpart to the release-time enforcement in `assembleReleasePlan`, which
 * only checks packages a plan actually releases — so a committed manifest that
 * drifted out of band, or a fixed group that fell out of lockstep, would
 * otherwise go unnoticed until a release that happens to touch it. Returns
 * every violation so a caller can report them all at once; malformed
 * configuration (unknown lead, epic overlap, a group straddling an epic) still
 * throws, exactly as plan assembly would.
 */
export function checkVersioningInvariants (opts: CheckVersioningInvariantsOptions): VersioningInvariantViolation[] {
  const { participants, fixedGroups, epics } = resolveWorkspaceVersioning(opts)
  return [
    ...collectEpicBandViolations(epics, participants),
    ...collectFixedGroupViolations(fixedGroups, participants, opts.versioning),
  ]
}

function collectEpicBandViolations (epics: ResolvedEpic[], participants: Map<string, Participant>): VersioningInvariantViolation[] {
  // With no plan (no new versions), the band derives from the lead's current
  // major, and members are checked against their current versions.
  const noNewVersions = new Map<string, string>()
  const violations: VersioningInvariantViolation[] = []
  for (const epic of epics) {
    const band = epicBand(epic, participants, noNewVersions)
    for (const memberDir of [...epic.memberDirs].sort()) {
      const member = participants.get(memberDir)!
      const memberMajor = Number(member.currentVersion.split('.')[0])
      if (band.contains(memberMajor)) continue
      violations.push({
        code: 'VERSIONING_EPIC_OUT_OF_BAND',
        message: `${member.name} is at ${member.currentVersion}, whose major ${memberMajor} is outside the band ${band.low}-${band.high} of the epic led by "${epic.leadRef}" (major ${band.major}).`,
      })
    }
  }
  return violations
}

function collectFixedGroupViolations (
  fixedGroups: string[][],
  participants: Map<string, Participant>,
  versioning?: VersioningSettings
): VersioningInvariantViolation[] {
  const violations: VersioningInvariantViolation[] = []
  for (const [index, group] of fixedGroups.entries()) {
    const members = group.map((dir) => participants.get(dir)!)
    if (new Set(members.map((member) => member.currentVersion)).size > 1) {
      const detail = members.map((member) => `${member.name}@${member.currentVersion}`).join(', ')
      violations.push({
        code: 'VERSIONING_FIXED_GROUP_MISMATCH',
        message: `The fixed group [${(versioning?.fixed ?? [])[index].join(', ')}] is not in lockstep: ${detail}.`,
      })
    }
  }
  return violations
}
