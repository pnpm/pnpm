import path from 'node:path'

import type { DepPath, PkgResolutionId } from '@pnpm/types'
import normalize from 'normalize-path'

import { isCompatibleAndHasMoreDeps } from './depPathCompatibility.js'
import type { NodeId } from './nextNodeId.js'
import type { LinkedDependency } from './resolveDependencies.js'
import type { ResolvedDirectDependency, ResolvedImporters } from './resolveDependencyTree.js'
import type {
  DependenciesByProjectId,
  GenericDependenciesGraphWithResolvedChildren,
  PartialResolvedPackage,
  ProjectToResolve,
} from './resolvePeers.js'

export interface DedupeInjectedDepsOptions<Pkg extends PartialResolvedPackage> {
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
  dependenciesByProjectId: DependenciesByProjectId
  lockfileDir: string
  pathsByNodeId: Map<NodeId, DepPath>
  projects: ProjectToResolve[]
  resolvedImporters: ResolvedImporters
  workspaceProjectIds: Set<string>
}

export function dedupeInjectedDeps<Pkg extends PartialResolvedPackage> (
  opts: DedupeInjectedDepsOptions<Pkg>
): void {
  const injectedDepsByProjects = getInjectedDepsByProjects(opts)
  const dedupeMap = getDedupeMap(injectedDepsByProjects, opts)
  applyDedupeMap(dedupeMap, opts)
}

type InjectedDepsByProjects = Map<string, Map<string, { depPath: DepPath, id: string }>>

function getInjectedDepsByProjects<Pkg extends PartialResolvedPackage> (
  opts: Pick<DedupeInjectedDepsOptions<Pkg>, 'projects' | 'pathsByNodeId' | 'depGraph' | 'workspaceProjectIds'>
): InjectedDepsByProjects {
  const injectedDepsByProjects = new Map<string, Map<string, { depPath: DepPath, id: string }>>()
  for (const project of opts.projects) {
    const injectedDeps = getInjectedWorkspaceDepsOfProject(project, opts)
    if (injectedDeps.size > 0) {
      injectedDepsByProjects.set(project.id, injectedDeps)
    }
  }
  return injectedDepsByProjects
}

function getInjectedWorkspaceDepsOfProject<Pkg extends PartialResolvedPackage> (
  project: ProjectToResolve,
  opts: Pick<DedupeInjectedDepsOptions<Pkg>, 'pathsByNodeId' | 'depGraph' | 'workspaceProjectIds'>
): Map<string, { depPath: DepPath, id: string }> {
  const injectedDeps = new Map<string, { depPath: DepPath, id: string }>()
  for (const [alias, nodeId] of project.directNodeIdsByAlias.entries()) {
    const depPath = opts.pathsByNodeId.get(nodeId)!
    if (!opts.depGraph[depPath].id.startsWith('file:')) continue
    const id = opts.depGraph[depPath].id.substring(5)
    if (opts.workspaceProjectIds.has(id)) {
      injectedDeps.set(alias, { depPath, id })
    }
  }
  return injectedDeps
}

type DedupeMap = Map<string, Map<string, string>>

type DedupeMapOptions<Pkg extends PartialResolvedPackage> = Pick<DedupeInjectedDepsOptions<Pkg>, 'depGraph' | 'dependenciesByProjectId'>

function getDedupeMap<Pkg extends PartialResolvedPackage> (
  injectedDepsByProjects: InjectedDepsByProjects,
  opts: DedupeMapOptions<Pkg>
): DedupeMap {
  const toDedupe = new Map<string, Map<string, string>>()
  for (const [id, deps] of injectedDepsByProjects.entries()) {
    const dedupedInjectedDeps = new Map<string, string>()
    for (const [alias, dep] of deps.entries()) {
      if (canDedupeInjectedDep(dep, opts)) {
        dedupedInjectedDeps.set(alias, dep.id)
      }
    }
    toDedupe.set(id, dedupedInjectedDeps)
  }
  return toDedupe
}

function canDedupeInjectedDep<Pkg extends PartialResolvedPackage> (
  dep: { depPath: DepPath, id: string },
  opts: DedupeMapOptions<Pkg>
): boolean {
  const node = opts.depGraph[dep.depPath]
  const targetProjectDeps = opts.dependenciesByProjectId[dep.id]
  // In single-project operations (e.g. `pnpm rm` from inside a workspace package) the target
  // workspace project isn't being resolved, so its children aren't in
  // `dependenciesByProjectId`. The injected dep was resolved against the same workspace
  // package source, so dedupe is safe. The exception is peer-suffixed depPaths, whose
  // resolution depends on the importer's peer context. A plain `link:` would lose that, so
  // we skip dedupe for those. A depPath is `${pkgIdWithPatchHash}${peerDepGraphHash}`, so it
  // carries a peer suffix exactly when it differs from its peer-free `pkgIdWithPatchHash`.
  if (!targetProjectDeps) {
    return (node.pkgIdWithPatchHash as string) === dep.depPath
  }
  // Check for subgroup not equal.
  // The injected project in the workspace may have dev deps
  return Object.entries(node.children)
    .every(([alias, depPath]) => isChildProvidedByTargetProject(opts.depGraph, targetProjectDeps.get(alias), depPath))
}

function isChildProvidedByTargetProject<Pkg extends PartialResolvedPackage> (
  depGraph: DedupeMapOptions<Pkg>['depGraph'],
  targetDepPath: DepPath | undefined,
  depPath: DepPath
): boolean {
  if (targetDepPath === depPath) return true
  if (targetDepPath == null) return false
  // A shared dep can resolve peer-suffixed on one side and peer-free on
  // the other (e.g. an existing lockfile pinned debug's optional
  // supports-color for the target project but not the injected
  // occurrence). Accept the target's variant when it's the same package
  // identity and a compatible superset. See pnpm/pnpm#10433.
  const targetNode = depGraph[targetDepPath]
  const injectedChildNode = depGraph[depPath]
  if (targetNode == null || injectedChildNode == null) return false
  if (targetNode.pkgIdWithPatchHash !== injectedChildNode.pkgIdWithPatchHash) return false
  return isCompatibleAndHasMoreDeps(depGraph, targetDepPath, depPath)
}

function applyDedupeMap<Pkg extends PartialResolvedPackage> (
  dedupeMap: DedupeMap,
  opts: Pick<DedupeInjectedDepsOptions<Pkg>, 'dependenciesByProjectId' | 'resolvedImporters' | 'lockfileDir'>
): void {
  for (const [id, aliases] of dedupeMap.entries()) {
    for (const [alias, dedupedProjectId] of aliases.entries()) {
      opts.dependenciesByProjectId[id].delete(alias)
      const index = opts.resolvedImporters[id].directDependencies.findIndex((dep) => dep.alias === alias)
      const prev = opts.resolvedImporters[id].directDependencies[index]
      const linkedDep: LinkedDependency & ResolvedDirectDependency = {
        ...prev,
        pkg: prev,
        isLinkedDependency: true,
        pkgId: `link:${normalize(path.relative(id, dedupedProjectId))}` as PkgResolutionId,
        resolution: {
          type: 'directory',
          directory: path.join(opts.lockfileDir, dedupedProjectId),
        },
      }
      opts.resolvedImporters[id].directDependencies[index] = linkedDep
      opts.resolvedImporters[id].linkedDependencies.push(linkedDep)
    }
  }
}
