import path from 'node:path'

import { DepType } from '@pnpm/lockfile.detect-dep-types'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'
import type { DependenciesField, ProjectId } from '@pnpm/types'

import { buildPurl } from './purl.js'
import type { SbomComponent, SbomRelationship } from './types.js'

export interface WorkspacePackageInfo {
  name: string
  version: string
  license?: string
  description?: string
  author?: string
  repository?: string
}

interface WorkspaceLink {
  sourceImporterId: ProjectId
  targetImporterId: ProjectId
  depName: string
  devOnly: boolean
}

type IncludedDependencyFields = { [dependenciesField in DependenciesField]: boolean }

export function resolveWorkspaceDeps (
  lockfile: LockfileObject,
  importerIds: ProjectId[],
  include?: IncludedDependencyFields
): { links: WorkspaceLink[], additionalImporterIds: ProjectId[] } {
  const links: WorkspaceLink[] = []
  const visited = new Set<string>(importerIds)
  const queue = [...importerIds]
  const additionalImporterIds: ProjectId[] = []

  for (let head = 0; head < queue.length; head++) {
    const importerId = queue[head]
    const snapshot = lockfile.importers[importerId]
    if (!snapshot) continue

    for (const link of findImporterLinks(lockfile, { importerId, snapshot, include })) {
      links.push(link)
      const targetId = link.targetImporterId
      if (!visited.has(targetId)) {
        visited.add(targetId)
        additionalImporterIds.push(targetId)
        queue.push(targetId)
      }
    }
  }

  return { links, additionalImporterIds }
}

function findImporterLinks (
  lockfile: LockfileObject,
  { importerId, snapshot, include }: {
    importerId: ProjectId
    snapshot: ProjectSnapshot
    include?: IncludedDependencyFields
  }
): WorkspaceLink[] {
  const devDepNames = new Set(Object.keys(snapshot.devDependencies ?? {}))
  const prodDeps = {
    ...(include?.dependencies !== false ? snapshot.dependencies : {}),
    ...(include?.optionalDependencies !== false ? snapshot.optionalDependencies : {}),
  }
  const allDeps: Record<string, string> = {
    ...prodDeps,
    ...(include?.devDependencies !== false ? snapshot.devDependencies : {}),
  }
  const links: WorkspaceLink[] = []
  for (const [depName, reference] of Object.entries(allDeps)) {
    const targetId = resolveLinkedImporterId(lockfile, importerId, reference)
    if (targetId == null) continue
    const devOnly = devDepNames.has(depName) && !(depName in prodDeps)
    links.push({ sourceImporterId: importerId, targetImporterId: targetId, depName, devOnly })
  }
  return links
}

function resolveLinkedImporterId (lockfile: LockfileObject, importerId: ProjectId, reference: string): ProjectId | undefined {
  if (!reference.startsWith('link:')) return undefined

  const linkPath = reference.slice(5)
  const targetId = path.posix.normalize(
    importerId === ('.' as ProjectId) ? linkPath : path.posix.join(importerId, linkPath)
  ) as ProjectId

  // A crafted lockfile can point a `link:` target outside the workspace root;
  // such importer IDs must never be followed, as they later become filesystem reads.
  if (path.posix.isAbsolute(targetId) || targetId === '..' || targetId.startsWith('../')) return undefined

  // `in` would also match inherited keys (e.g. "toString"); a crafted lockfile
  // must not be able to enqueue importer IDs that are not actually present.
  if (!Object.prototype.hasOwnProperty.call(lockfile.importers, targetId)) return undefined

  return targetId
}

export interface WorkspaceComponentsTarget {
  componentsMap: Map<string, SbomComponent>
  relationships: SbomRelationship[]
}

/**
 * Adds the workspace packages that the included importers link to as
 * components, with a relationship from the package that links each one.
 */
export function addWorkspaceComponents (
  target: WorkspaceComponentsTarget,
  { workspacePackages, links, importerIdSet, rootPurl }: {
    workspacePackages: Record<ProjectId, WorkspacePackageInfo>
    links: WorkspaceLink[]
    importerIdSet: Set<string>
    rootPurl: string
  }
): void {
  const workspaceDepTypes = getWorkspaceDepTypes(workspacePackages, links)
  for (const dep of links) {
    const info = workspacePackages[dep.targetImporterId]
    if (!info) continue

    const purl = buildPurl({ name: info.name, version: info.version })
    const parentPurl = importerIdSet.has(dep.sourceImporterId)
      ? rootPurl
      : getWorkspacePackagePurl(workspacePackages, dep.sourceImporterId) ?? rootPurl
    target.relationships.push({ from: parentPurl, to: purl })

    if (!target.componentsMap.has(purl)) {
      target.componentsMap.set(purl, {
        name: info.name,
        version: info.version,
        purl,
        depPath: `link:${dep.targetImporterId}`,
        depType: workspaceDepTypes.get(purl) ?? DepType.ProdOnly,
        license: info.license,
        description: info.description,
        author: info.author,
        repository: info.repository,
      })
    }
  }
}

function getWorkspaceDepTypes (
  workspacePackages: Record<ProjectId, WorkspacePackageInfo>,
  links: WorkspaceLink[]
): Map<string, DepType> {
  const workspaceDepTypes = new Map<string, DepType>()
  for (const dep of links) {
    const purl = getWorkspacePackagePurl(workspacePackages, dep.targetImporterId)
    if (purl == null) continue
    if (!dep.devOnly) {
      workspaceDepTypes.set(purl, DepType.ProdOnly)
    } else if (!workspaceDepTypes.has(purl)) {
      workspaceDepTypes.set(purl, DepType.DevOnly)
    }
  }
  return workspaceDepTypes
}

function getWorkspacePackagePurl (
  workspacePackages: Record<ProjectId, WorkspacePackageInfo>,
  importerId: ProjectId
): string | undefined {
  const info = workspacePackages[importerId]
  return info ? buildPurl({ name: info.name, version: info.version }) : undefined
}
