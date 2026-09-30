import { PnpmError } from '@pnpm/error'
import type { ProjectManifest, VersioningSettings } from '@pnpm/types'
import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'
import { valid, validRange } from 'semver'

import { type ProjectRefIndex, resolveConfigRef, toProjectDir, type WorkspaceProject } from './projectRefs.js'

const PROPAGATED_DEP_FIELDS = ['dependencies', 'optionalDependencies', 'peerDependencies'] as const

type PropagatedDepField = typeof PROPAGATED_DEP_FIELDS[number]

export interface Participant {
  name: string
  dir: string
  rootDir: string
  currentVersion: string
  manifest: ProjectManifest
  /** Workspace-internal production dependencies, by target project dir. */
  internalDeps: InternalDep[]
}

export interface InternalDep {
  targetDir: string
  targetName: string
  fieldName: PropagatedDepField
  alias: string
  spec: string
}

export interface CollectParticipantsOptions {
  workspaceDir: string
  versioning?: VersioningSettings
}

export function collectParticipants (
  projects: WorkspaceProject[],
  refs: ProjectRefIndex,
  opts: CollectParticipantsOptions
): Map<string, Participant> {
  const ignoredDirs = collectIgnoredDirs(refs, opts.versioning)
  const participants = new Map<string, Participant>()
  for (const project of projects) {
    const participant = toParticipant(project, opts.workspaceDir)
    if (participant == null || ignoredDirs.has(participant.dir)) continue
    participants.set(participant.dir, participant)
  }
  for (const participant of participants.values()) {
    participant.internalDeps = collectInternalDeps(participant, { refs, participants })
  }
  return participants
}

function collectIgnoredDirs (refs: ProjectRefIndex, versioning?: VersioningSettings): Set<string> {
  const ignoredDirs = new Set<string>()
  for (const ref of versioning?.ignore ?? []) {
    for (const dir of resolveConfigRef(refs, ref, 'versioning.ignore')) {
      ignoredDirs.add(dir)
    }
  }
  return ignoredDirs
}

function toParticipant (project: WorkspaceProject, workspaceDir: string): Participant | undefined {
  const { name, version } = project.manifest
  const dir = toProjectDir(workspaceDir, project.rootDir)
  // What cannot release is excluded automatically: unnamed and versionless
  // (private) packages, packages with non-semver placeholder versions, and
  // the explicitly frozen ones.
  if (name == null || version == null || valid(version) == null) return undefined
  return {
    name,
    dir,
    rootDir: project.rootDir,
    currentVersion: version,
    manifest: project.manifest,
    internalDeps: [],
  }
}

interface ParticipantLookup {
  refs: ProjectRefIndex
  participants: Map<string, Participant>
}

function collectInternalDeps (participant: Participant, lookup: ParticipantLookup): InternalDep[] {
  const internalDeps: InternalDep[] = []
  for (const fieldName of PROPAGATED_DEP_FIELDS) {
    for (const [alias, spec] of Object.entries(participant.manifest[fieldName] ?? {})) {
      const internalDep = resolveInternalDep(participant, { fieldName, alias, spec }, lookup)
      if (internalDep != null) {
        internalDeps.push(internalDep)
      }
    }
  }
  return internalDeps
}

function resolveInternalDep (
  participant: Participant,
  declaration: Pick<InternalDep, 'fieldName' | 'alias' | 'spec'>,
  { refs, participants }: ParticipantLookup
): InternalDep | undefined {
  const targetName = internalDepTargetName(declaration.alias, declaration.spec, refs)
  if (targetName == null) return undefined
  const targetDirs = refs.nameToDirs(targetName).filter((dir) => participants.has(dir))
  if (targetDirs.length === 0) return undefined
  // A workspace: range naming an ambiguous package cannot be linked at
  // install time, so the release engine never legitimately sees one.
  if (targetDirs.length > 1) {
    throw new PnpmError(
      'VERSIONING_AMBIGUOUS_PACKAGE',
      `Package ${participant.name} (./${participant.dir}) depends on ${targetName}, which matches multiple workspace projects: ${targetDirs.map((dir) => `./${dir}`).join(', ')}`
    )
  }
  return { targetDir: targetDirs[0], targetName, ...declaration }
}

/**
 * Decides whether a dependency entry points at a workspace package. Aliased
 * specs targeting somewhere else (`npm:`, `file:`, git URLs, ...) are external
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

export function assertInternalDepsUseWorkspaceProtocol (participants: Map<string, Participant>): void {
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
