import { packageManifestLogger } from '@pnpm/core-loggers'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import type { DepPath, ProjectManifest } from '@pnpm/types'

import { addDirectDependenciesToLockfile } from './addDirectDependenciesToLockfile.js'
import { depPathToRef } from './depPathToRef.js'
import type { DependenciesGraph, ImporterToResolve } from './index.js'
import type { LinkedDependency, ResolvedImporters } from './resolveDependencyTree.js'
import type { DependenciesByProjectId } from './resolvePeers.js'
import type { ResolveImporter } from './toResolveImporter.js'
import { updateProjectManifest } from './updateProjectManifest.js'

export interface UpdateLockfileImportersOptions {
  autoInstallPeers?: boolean
  dedupeInjectedDeps?: boolean
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
  // Capture previous importer refs before the lockfile importer is rebuilt,
  // so an install that doesn't actually change a workspace dependency (e.g.
  // updating an unrelated dependency) does not rewrite its `link:` entry to a
  // peer-suffixed `file:`.
  const previousDirectRefs = getDirectRefs(opts.wantedLockfile.importers[project.id])
  const updateMatching = opts.importers[opts.index].updateMatching
  const updateTargetedAliases = getUpdateTargetedAliases(project)
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

  writeDirectDependencyRefs(project, {
    ...opts,
    isTargetedByUpdate: (alias, depName) => updateTargetedAliases.has(alias) || (updateMatching?.(depName) ?? false),
    previousDirectRefs,
  })
}

function writeDirectDependencyRefs (
  project: ResolveImporter,
  opts: UpdateLockfileImportersOptions & {
    isTargetedByUpdate: (alias: string, depName: string) => boolean
    previousDirectRefs: Record<string, string>
  }
): void {
  // Every project records its manifest's `dependenciesMeta`, including one
  // whose dependencies all resolved to workspace links and so has no entry
  // in `dependenciesByProjectId`.
  const projectSnapshot = opts.wantedLockfile.importers[project.id]
  if (project.manifest.dependenciesMeta != null) {
    projectSnapshot.dependenciesMeta = project.manifest.dependenciesMeta
  }
  for (const [alias, depPath] of opts.dependenciesByProjectId[project.id].entries()) {
    const depName = opts.dependenciesGraph[depPath].name
    const ref = getDirectDependencyRef({
      alias,
      depName,
      depPath,
      isTargetedByUpdate: opts.isTargetedByUpdate(alias, depName),
      preserveDedupedWorkspaceLinks: Boolean(opts.dedupeInjectedDeps),
      previousRef: opts.previousDirectRefs[alias],
    })
    setDirectDependencyRef(projectSnapshot, { alias, ref })
  }
}

function getDirectRefs (importerSnapshot: ProjectSnapshot | undefined): Record<string, string> {
  return {
    ...importerSnapshot?.dependencies,
    ...importerSnapshot?.devDependencies,
    ...importerSnapshot?.optionalDependencies,
  }
}

/**
 * Aliases this run actually targets (added, spec-changed, or matched by a
 * `pnpm update <name>`). Only these may legitimately change their
 * `link:`/`file:` form; the preserve-prior-link guard in
 * `getDirectDependencyRef` is limited to dependencies outside this set.
 * `updateSpec` is deliberately not consulted: a plain install marks every
 * manifest dependency with it, so it signals "re-check the spec", not "the
 * user targeted this dependency".
 */
function getUpdateTargetedAliases (project: ResolveImporter): Set<string> {
  return new Set(
    project.wantedDependencies.flatMap(({ alias, bareSpecifier, isNew, prevSpecifier }) =>
      alias != null && (isNew === true || (prevSpecifier != null && bareSpecifier !== prevSpecifier))
        ? [alias]
        : []
    )
  )
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

function getDirectDependencyRef (
  opts: {
    alias: string
    depName: string
    depPath: DepPath
    isTargetedByUpdate: boolean
    preserveDedupedWorkspaceLinks: boolean
    previousRef: string | undefined
  }
): string {
  const ref = depPathToRef(opts.depPath, {
    alias: opts.alias,
    realName: opts.depName,
  })
  // A workspace dependency resolved to `link:` has no version to update, so
  // it should stay `link:` unless this run specifically targets it (a spec
  // change or `pnpm update <name>`). Preserving it stops an update of an
  // unrelated dependency (e.g. `pnpm update <other-pkg>`) from re-resolving
  // an untouched injected workspace dep and flipping its `link:` to a
  // peer-suffixed `file:` on paths dedupeInjectedDeps doesn't reach. See
  // pnpm/pnpm#10433.
  if (
    opts.preserveDedupedWorkspaceLinks &&
    !opts.isTargetedByUpdate &&
    ref.startsWith('file:') &&
    opts.previousRef?.startsWith('link:')
  ) {
    return opts.previousRef
  }
  return ref
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
