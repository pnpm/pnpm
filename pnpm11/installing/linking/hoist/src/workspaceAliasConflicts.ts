import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { ProjectId } from '@pnpm/types'

import type { DirectDependenciesByImporterId, HoistedWorkspaceProject, HoistGraphResult, HoistType } from './types.js'

export type WorkspacePlacementCandidate = [ProjectId, HoistedWorkspaceProject, HoistType]

interface AliasIndex {
  aliases: Set<string>
  ancestors: Set<string>
}

export interface OccupiedAliases {
  private: AliasIndex
  public: AliasIndex
}

export function findConflictingWorkspaceProjectIds (
  candidates: WorkspacePlacementCandidate[],
  occupiedAliases?: OccupiedAliases
): Set<ProjectId> {
  const conflictingProjectIds = new Set<ProjectId>()
  for (const hoistType of ['private', 'public'] as const) {
    const projectIdsByName = groupCandidateProjectIdsByName({ candidates, conflictingProjectIds, hoistType, occupiedAliases })
    addNestedAliasConflicts(projectIdsByName, conflictingProjectIds)
  }
  return conflictingProjectIds
}

/**
 * Groups the candidates of one hoist type by their lowercased name. A candidate
 * whose alias conflicts with an already occupied alias is marked as conflicting
 * and left out of the groups.
 */
function groupCandidateProjectIdsByName (opts: {
  candidates: WorkspacePlacementCandidate[]
  conflictingProjectIds: Set<ProjectId>
  hoistType: HoistType
  occupiedAliases?: OccupiedAliases
}): Map<string, ProjectId[]> {
  const projectIdsByName = new Map<string, ProjectId[]>()
  for (const [projectId, project, candidateHoistType] of opts.candidates) {
    if (candidateHoistType !== opts.hoistType) continue
    const name = project.name.toLowerCase()
    if (opts.occupiedAliases != null && conflictsWithOccupiedAlias(name, opts.occupiedAliases[opts.hoistType])) {
      opts.conflictingProjectIds.add(projectId)
      continue
    }
    const projectIds = projectIdsByName.get(name)
    if (projectIds == null) {
      projectIdsByName.set(name, [projectId])
    } else {
      projectIds.push(projectId)
    }
  }
  return projectIdsByName
}

function addNestedAliasConflicts (projectIdsByName: Map<string, ProjectId[]>, conflictingProjectIds: Set<ProjectId>): void {
  const activeAncestors: Array<[string, ProjectId[]]> = []
  for (const current of [...projectIdsByName.entries()].sort(([left], [right]) => compareWorkspaceAliases(left, right))) {
    while (activeAncestors.length > 0 && !current[0].startsWith(`${activeAncestors.at(-1)![0]}/`)) {
      activeAncestors.pop()
    }
    if (activeAncestors.length > 0) {
      addProjectIds([...activeAncestors, current], conflictingProjectIds)
    }
    activeAncestors.push(current)
  }
}

function addProjectIds (groups: Array<[string, ProjectId[]]>, projectIds: Set<ProjectId>): void {
  for (const [, groupProjectIds] of groups) {
    for (const projectId of groupProjectIds) projectIds.add(projectId)
  }
}

export function collectOccupiedAliases<NodeId extends string> (
  result: HoistGraphResult<NodeId> | null,
  directDepsByImporterId: DirectDependenciesByImporterId<NodeId>
): OccupiedAliases {
  const occupiedAliases: OccupiedAliases = {
    private: { aliases: new Set(), ancestors: new Set() },
    public: { aliases: new Set(), ancestors: new Set() },
  }
  if (result != null) {
    for (const aliases of result.hoistedDependenciesByNodeId.values()) {
      for (const [alias, hoistType] of Object.entries(aliases)) {
        addOccupiedAlias(alias, occupiedAliases[hoistType])
      }
    }
  }
  const rootDirectDeps = directDepsByImporterId['.' as ProjectId]
  if (rootDirectDeps != null) {
    for (const alias of rootDirectDeps.keys()) addOccupiedAlias(alias, occupiedAliases.public)
  }
  return occupiedAliases
}

function addOccupiedAlias (alias: string, index: AliasIndex): void {
  const components = alias.toLowerCase().split('/')
  index.aliases.add(components.join('/'))
  for (let length = 1; length < components.length; length++) {
    index.ancestors.add(components.slice(0, length).join('/'))
  }
}

function conflictsWithOccupiedAlias (alias: string, index: AliasIndex): boolean {
  if (index.ancestors.has(alias)) return true
  const components = alias.split('/')
  for (let length = 1; length < components.length; length++) {
    if (index.aliases.has(components.slice(0, length).join('/'))) return true
  }
  return false
}

function compareWorkspaceAliases (left: string, right: string): number {
  const leftComponents = left.split('/')
  const rightComponents = right.split('/')
  const length = Math.min(leftComponents.length, rightComponents.length)
  for (let index = 0; index < length; index++) {
    const compared = lexCompare(leftComponents[index], rightComponents[index])
    if (compared !== 0) return compared
  }
  return leftComponents.length - rightComponents.length
}
