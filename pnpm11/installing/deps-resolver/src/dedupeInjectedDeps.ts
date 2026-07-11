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

export interface DedupeInjectedDepsOptions<T extends PartialResolvedPackage> {
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>
  dependenciesByProjectId: DependenciesByProjectId
  lockfileDir: string
  pathsByNodeId: Map<NodeId, DepPath>
  projects: ProjectToResolve[]
  resolvedImporters: ResolvedImporters
  workspaceProjectIds: Set<string>
}

export function dedupeInjectedDeps<T extends PartialResolvedPackage> (
  opts: DedupeInjectedDepsOptions<T>
): void {
  const injectedDepsByProjects = getInjectedDepsByProjects(opts)
  const dedupeMap = getDedupeMap(injectedDepsByProjects, opts)
  applyDedupeMap(dedupeMap, opts)
}

type InjectedDepsByProjects = Map<string, Map<string, { depPath: DepPath, id: string }>>

function getInjectedDepsByProjects<T extends PartialResolvedPackage> (
  opts: Pick<DedupeInjectedDepsOptions<T>, 'projects' | 'pathsByNodeId' | 'depGraph' | 'workspaceProjectIds'>
): InjectedDepsByProjects {
  const injectedDepsByProjects = new Map<string, Map<string, { depPath: DepPath, id: string }>>()
  for (const project of opts.projects) {
    for (const [alias, nodeId] of project.directNodeIdsByAlias.entries()) {
      const depPath = opts.pathsByNodeId.get(nodeId)!
      if (!opts.depGraph[depPath].id.startsWith('file:')) continue
      const id = opts.depGraph[depPath].id.substring(5)
      if (opts.workspaceProjectIds.has(id)) {
        if (!injectedDepsByProjects.has(project.id)) injectedDepsByProjects.set(project.id, new Map())
        injectedDepsByProjects.get(project.id)!.set(alias, { depPath, id })
      }
    }
  }
  return injectedDepsByProjects
}

type DedupeMap = Map<string, Map<string, string>>

function getDedupeMap<T extends PartialResolvedPackage> (
  injectedDepsByProjects: InjectedDepsByProjects,
  opts: Pick<DedupeInjectedDepsOptions<T>, 'depGraph' | 'dependenciesByProjectId'>
): DedupeMap {
  const toDedupe = new Map<string, Map<string, string>>()
  for (const [id, deps] of injectedDepsByProjects.entries()) {
    const dedupedInjectedDeps = new Map<string, string>()
    for (const [alias, dep] of deps.entries()) {
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
        if ((node.pkgIdWithPatchHash as string) === dep.depPath) {
          dedupedInjectedDeps.set(alias, dep.id)
        }
        continue
      }
      // Check for subgroup not equal.
      // The injected project in the workspace may have dev deps
      const children = Object.entries(node.children)
      const isSubset = children
        .every(([alias, depPath]) => {
          const targetDepPath = targetProjectDeps.get(alias)
          if (targetDepPath === depPath) return true
          if (targetDepPath == null) return false
          // An ordinary (non-workspace) shared dependency of the injected
          // package can resolve to a peer-suffixed variant on one side and a
          // peer-free variant on the other -- e.g. when reconciling against an
          // existing lockfile pins an optional peer (debug's supports-color)
          // for the target project's own resolution but not for the injected
          // occurrence. Both are valid resolutions of the same package; what
          // matters is whether the target project's own copy is at least as
          // complete as what the injected occurrence needed, not that the two
          // depPath strings are byte-identical. See pnpm/pnpm#10433.
          //
          // Only tolerate this when both sides are the *same package identity*
          // (same `pkgIdWithPatchHash`, i.e. differing only by peer suffix).
          // `isCompatibleAndHasMoreDeps` compares dependency/peer sets, not
          // identity, so without this guard two different versions of a shared
          // dep -- leaf packages especially, whose sets are both empty -- would
          // be treated as interchangeable and wrongly deduped.
          const targetNode = opts.depGraph[targetDepPath]
          const injectedChildNode = opts.depGraph[depPath]
          if (targetNode == null || injectedChildNode == null) return false
          if (targetNode.pkgIdWithPatchHash !== injectedChildNode.pkgIdWithPatchHash) return false
          return isCompatibleAndHasMoreDeps(opts.depGraph, targetDepPath, depPath)
        })
      if (isSubset) {
        dedupedInjectedDeps.set(alias, dep.id)
      }
    }
    toDedupe.set(id, dedupedInjectedDeps)
  }
  return toDedupe
}

function applyDedupeMap<T extends PartialResolvedPackage> (
  dedupeMap: DedupeMap,
  opts: Pick<DedupeInjectedDepsOptions<T>, 'dependenciesByProjectId' | 'resolvedImporters' | 'lockfileDir'>
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
