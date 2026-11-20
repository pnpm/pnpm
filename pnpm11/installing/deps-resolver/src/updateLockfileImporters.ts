import { packageManifestLogger } from '@pnpm/core-loggers'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import type { ProjectManifest } from '@pnpm/types'

import { addDirectDependenciesToLockfile } from './addDirectDependenciesToLockfile.js'
import { depPathToRef } from './depPathToRef.js'
import type { DependenciesGraph, ImporterToResolve } from './index.js'
import type { LinkedDependency, ResolvedImporters } from './resolveDependencyTree.js'
import type { DependenciesByProjectId } from './resolvePeers.js'
import type { ResolveImporter } from './toResolveImporter.js'
import { updateProjectManifest } from './updateProjectManifest.js'

export interface UpdateLockfileImportersOptions {
  autoInstallPeers?: boolean
  dependenciesByProjectId: DependenciesByProjectId
  dependenciesGraph: DependenciesGraph
  excludeLinksFromLockfile?: boolean
  importers: ImporterToResolve[]
  preserveWorkspaceProtocol: boolean
  resolvedImporters: ResolvedImporters
  saveWorkspaceProtocol: 'rolling' | boolean
  wantedLockfile: LockfileObject
}

/**
 * Updates the manifests of the projects and writes their direct dependencies
 * to the importers of the wanted lockfile. Returns the linked dependencies of
 * each project.
 */
export async function updateLockfileImporters (
  projectsToResolve: ResolveImporter[],
  opts: UpdateLockfileImportersOptions
): Promise<Record<string, LinkedDependency[]>> {
  const linkedDependenciesByProjectId: Record<string, LinkedDependency[]> = {}
  await Promise.all(projectsToResolve.map(async (project, index) => {
    linkedDependenciesByProjectId[project.id] = opts.resolvedImporters[project.id].linkedDependencies
    await updateLockfileImporter(project, { ...opts, index })
  }))
  return linkedDependenciesByProjectId
}

async function updateLockfileImporter (
  project: ResolveImporter,
  opts: UpdateLockfileImportersOptions & { index: number }
): Promise<void> {
  const resolvedImporter = opts.resolvedImporters[project.id]
  const [updatedManifest, updatedOriginalManifest] = await getUpdatedManifests(project, opts)

  if (updatedManifest != null) {
    if (opts.autoInstallPeers) {
      addPeersToDependencies(updatedManifest)
    }
    const projectSnapshot = opts.wantedLockfile.importers[project.id]
    opts.wantedLockfile.importers[project.id] = addDirectDependenciesToLockfile({
      newManifest: updatedManifest,
      projectSnapshot,
      linkedPackages: resolvedImporter.linkedDependencies,
      directDependencies: resolvedImporter.directDependencies,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    })
  }

  opts.importers[opts.index].manifest = updatedOriginalManifest ?? project.originalManifest ?? project.manifest

  writeDirectDependencyRefs(project, opts)
}

function writeDirectDependencyRefs (
  project: ResolveImporter,
  opts: UpdateLockfileImportersOptions
): void {
  // Every project records its manifest's `dependenciesMeta`, including one
  // whose dependencies all resolved to workspace links and so has no entry
  // in `dependenciesByProjectId`.
  const projectSnapshot = opts.wantedLockfile.importers[project.id]
  if (project.manifest.dependenciesMeta != null) {
    projectSnapshot.dependenciesMeta = project.manifest.dependenciesMeta
  }
  for (const [alias, depPath] of opts.dependenciesByProjectId[project.id].entries()) {
    const ref = depPathToRef(depPath, {
      alias,
      realName: opts.dependenciesGraph[depPath].name,
    })
    setDirectDependencyRef(projectSnapshot, { alias, ref })
  }
}

async function getUpdatedManifests (
  project: ResolveImporter,
  opts: UpdateLockfileImportersOptions
): Promise<Array<ProjectManifest | undefined>> {
  if (project.updatePackageManifest) {
    return updateProjectManifest(project, {
      directDependencies: opts.resolvedImporters[project.id].directDependencies,
      preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    })
  }
  packageManifestLogger.debug({
    prefix: project.rootDir,
    updated: project.manifest,
  })
  return [project.manifest, project.originalManifest]
}

function addPeersToDependencies (manifest: ProjectManifest): void {
  if (!manifest.peerDependencies) return
  const allDeps = getAllDependenciesFromManifest(manifest)
  for (const [peerName, peerRange] of Object.entries(manifest.peerDependencies)) {
    if (allDeps[peerName]) continue
    manifest.dependencies ??= {}
    manifest.dependencies[peerName] = peerRange
  }
}

function setDirectDependencyRef (projectSnapshot: ProjectSnapshot, { alias, ref }: { alias: string, ref: string }): void {
  if (projectSnapshot.dependencies?.[alias]) {
    projectSnapshot.dependencies[alias] = ref
  } else if (projectSnapshot.devDependencies?.[alias]) {
    projectSnapshot.devDependencies[alias] = ref
  } else if (projectSnapshot.optionalDependencies?.[alias]) {
    projectSnapshot.optionalDependencies[alias] = ref
  }
}
