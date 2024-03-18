import '@total-typescript/ts-reset'
import path from 'node:path'
import { PnpmError } from '@pnpm/error'
import { filesIncludeInstallScripts } from '@pnpm/exec.files-include-install-scripts'
import { packageManifestLogger } from '@pnpm/core-loggers'
import { globalWarn } from '@pnpm/logger'
import type { Lockfile, ProjectSnapshot } from '@pnpm/lockfile-types'
import {
  getAllDependenciesFromManifest,
  getSpecFromPackageManifest,
  type PinnedVersion,
} from '@pnpm/manifest-utils'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import {
  type DependenciesField,
  DEPENDENCIES_FIELDS,
  type DependencyManifest,
  type ProjectManifest,
  type Registries,
} from '@pnpm/types'
import promiseShare from 'promise-share'
import difference from 'ramda/src/difference'
import zipWith from 'ramda/src/zipWith'
import isSubdir from 'is-subdir'
import {
  getWantedDependencies,
  type WantedDependency,
} from './getWantedDependencies'
import { depPathToRef } from './depPathToRef'
import {
  createNodeIdForLinkedLocalPkg,
  type UpdateMatchingFunction,
} from './resolveDependencies'
import {
  type Importer,
  type LinkedDependency,
  type ResolveDependenciesOptions,
  type ResolvedDirectDependency,
  type ResolvedPackage,
  resolveDependencyTree,
} from './resolveDependencyTree'
import {
  type GenericDependenciesGraph,
  type GenericDependenciesGraphNode,
  resolvePeers,
} from './resolvePeers'
import { toResolveImporter } from './toResolveImporter'
import { updateLockfile } from './updateLockfile'
import { updateProjectManifest } from './updateProjectManifest'

export type DependenciesGraph = GenericDependenciesGraph<ResolvedPackage>

export type DependenciesGraphNode = GenericDependenciesGraphNode &
  ResolvedPackage

export {
  getWantedDependencies,
  type LinkedDependency,
  type ResolvedPackage,
  type PinnedVersion,
  type UpdateMatchingFunction,
  type WantedDependency,
}

interface ProjectToLink {
  binsDir: string
  directNodeIdsByAlias: { [alias: string]: string }
  id: string
  linkedDependencies: LinkedDependency[]
  manifest: ProjectManifest
  modulesDir: string
  rootDir: string
  topParents: Array<{ name: string; version: string }>
}

export type ImporterToResolve = Importer<{
  isNew?: boolean
  nodeExecPath?: string
  pinnedVersion?: PinnedVersion
  raw: string
  updateSpec?: boolean
  preserveNonSemverVersionSpec?: boolean
}> & {
  peer?: boolean
  pinnedVersion?: PinnedVersion
  binsDir: string
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  update?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest: boolean
  targetDependenciesField?: DependenciesField
}

export async function resolveDependencies(
  importers: ImporterToResolve[],
  opts: ResolveDependenciesOptions & {
    defaultUpdateDepth: number
    dedupePeerDependents?: boolean
    dedupeDirectDeps?: boolean
    dedupeInjectedDeps?: boolean
    excludeLinksFromLockfile?: boolean
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: 'rolling' | boolean
    lockfileIncludeTarballUrl?: boolean
    allowNonAppliedPatches?: boolean
  }
) {
  const _toResolveImporter = toResolveImporter.bind(null, {
    defaultUpdateDepth: opts.defaultUpdateDepth,
    lockfileOnly: opts.dryRun,
    preferredVersions: opts.preferredVersions,
    virtualStoreDir: opts.virtualStoreDir,
    workspacePackages: opts.workspacePackages,
    updateToLatest: opts.updateToLatest,
    noDependencySelectors: importers.every(
      ({ wantedDependencies }) => wantedDependencies.length === 0
    ),
  })
  const projectsToResolve = await Promise.all(
    importers.map(async (project) => _toResolveImporter(project))
  )
  const {
    dependenciesTree,
    outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByDepPath,
    wantedToBeSkippedPackageIds,
    appliedPatches,
    time,
  } = await resolveDependencyTree(projectsToResolve, opts)

  // We only check whether patches were applied in cases when the whole lockfile was reanalyzed.
  if (
    opts.patchedDependencies &&
    (opts.forceFullResolution ||
      !Object.keys(opts.wantedLockfile.packages ?? {})?.length) &&
    Object.keys(opts.wantedLockfile.importers).length === importers.length
  ) {
    verifyPatches({
      patchedDependencies: Object.keys(opts.patchedDependencies),
      appliedPatches,
      allowNonAppliedPatches: opts.allowNonAppliedPatches,
    })
  }

  const projectsToLink = await Promise.all<ProjectToLink>(
    projectsToResolve.map(async (project) => {
      const resolvedImporter = resolvedImporters[project.id]

      const topParents: Array<{
        name: string
        version: string
        alias?: string
        linkedDir?: string
      }> = project.manifest
        ? await getTopParents(
          difference(
            Object.keys(getAllDependenciesFromManifest(project.manifest)),
            resolvedImporter.directDependencies.map(({ alias }) => alias) ||
                []
          ),
          project.modulesDir
        )
        : []
      resolvedImporter.linkedDependencies.forEach((linkedDependency) => {
        // The location of the external link may vary on different machines, so it is better not to include it in the lockfile.
        // As a workaround, we symlink to the root of node_modules, which is a symlink to the actual location of the external link.
        const target =
          !opts.excludeLinksFromLockfile ||
          isSubdir(opts.lockfileDir, linkedDependency.resolution.directory)
            ? linkedDependency.resolution.directory
            : path.join(project.modulesDir, linkedDependency.alias)
        const linkedDir = createNodeIdForLinkedLocalPkg(
          opts.lockfileDir,
          target
        )
        topParents.push({
          name: linkedDependency.alias,
          version: linkedDependency.version,
          linkedDir,
        })
      })

      return {
        binsDir: project.binsDir,
        directNodeIdsByAlias: resolvedImporter.directNodeIdsByAlias,
        id: project.id,
        linkedDependencies: resolvedImporter.linkedDependencies,
        manifest: project.manifest,
        modulesDir: project.modulesDir,
        rootDir: project.rootDir,
        topParents,
      }
    })
  )

  const {
    dependenciesGraph,
    dependenciesByProjectId,
    peerDependencyIssuesByProjects,
  } = resolvePeers({
    dependenciesTree,
    dedupePeerDependents: opts.dedupePeerDependents,
    dedupeInjectedDeps: opts.dedupeInjectedDeps,
    lockfileDir: opts.lockfileDir,
    projects: projectsToLink,
    virtualStoreDir: opts.virtualStoreDir,
    resolvePeersFromWorkspaceRoot: Boolean(opts.resolvePeersFromWorkspaceRoot),
    resolvedImporters,
  })

  const linkedDependenciesByProjectId: Record<string, LinkedDependency[]> = {}
  await Promise.all(
    projectsToResolve.map(async (project, index) => {
      const resolvedImporter = resolvedImporters[project.id]
      linkedDependenciesByProjectId[project.id] =
        resolvedImporter.linkedDependencies
      let updatedManifest: ProjectManifest | undefined
      let updatedOriginalManifest: ProjectManifest | undefined
      if (project.updatePackageManifest) {
        ;[updatedManifest, updatedOriginalManifest] =
          await updateProjectManifest(project, {
            directDependencies: resolvedImporter.directDependencies,
            preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
            saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
          })
      } else {
        updatedManifest = project.manifest
        updatedOriginalManifest = project.originalManifest
        packageManifestLogger.debug({
          prefix: project.rootDir,
          updated: project.manifest,
        })
      }

      if (updatedManifest != null) {
        if (opts.autoInstallPeers) {
          if (updatedManifest.peerDependencies) {
            const allDeps = getAllDependenciesFromManifest(updatedManifest)
            for (const [peerName, peerRange] of Object.entries(
              updatedManifest.peerDependencies
            )) {
              if (allDeps[peerName]) continue
              updatedManifest.dependencies ??= {}
              updatedManifest.dependencies[peerName] = peerRange
            }
          }
        }
        const projectSnapshot = opts.wantedLockfile.importers[project.id]
        opts.wantedLockfile.importers[project.id] =
          addDirectDependenciesToLockfile(
            updatedManifest,
            projectSnapshot,
            resolvedImporter.linkedDependencies,
            resolvedImporter.directDependencies,
            opts.registries,
            opts.excludeLinksFromLockfile
          )
      }

      importers[index].manifest =
        updatedOriginalManifest ?? project.originalManifest ?? project.manifest

      for (const [alias, depPath] of Object.entries(
        dependenciesByProjectId[project.id]
      )) {
        const projectSnapshot = opts.wantedLockfile.importers[project.id]
        if (typeof project.manifest.dependenciesMeta !== 'undefined') {
          projectSnapshot.dependenciesMeta = project.manifest.dependenciesMeta
        }

        const depNode = dependenciesGraph[depPath]

        const ref = depPathToRef(depPath, {
          alias,
          realName: depNode.name,
          registries: opts.registries,
          resolution: depNode.resolution,
        })
        if (projectSnapshot.dependencies?.[alias]) {
          projectSnapshot.dependencies[alias] = ref
        } else if (projectSnapshot.devDependencies?.[alias]) {
          projectSnapshot.devDependencies[alias] = ref
        } else if (projectSnapshot.optionalDependencies?.[alias]) {
          projectSnapshot.optionalDependencies[alias] = ref
        }
      }
    })
  )

  if (opts.dedupeDirectDeps) {
    const rootDeps = dependenciesByProjectId['.']
    if (rootDeps) {
      for (const [id, deps] of Object.entries(dependenciesByProjectId)) {
        if (id === '.') continue
        for (const [alias, depPath] of Object.entries(deps)) {
          if (depPath === rootDeps[alias]) {
            delete deps[alias]
          }
        }
      }
    }
  }

  const { newLockfile, pendingRequiresBuilds } = updateLockfile({
    dependenciesGraph,
    lockfile: opts.wantedLockfile,
    prefix: opts.virtualStoreDir,
    registries: opts.registries,
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
  })
  if (time) {
    newLockfile.time = {
      ...opts.wantedLockfile.time,
      ...time,
    }
  }

  if (opts.forceFullResolution && opts.wantedLockfile != null) {
    for (const [depPath, pkg] of Object.entries(dependenciesGraph)) {
      if (
        (opts.allowBuild != null && !opts.allowBuild(pkg.name)) ||
        opts.wantedLockfile.packages?.[depPath] == null ||
        pkg.requiresBuild === true
      )
        continue
      pendingRequiresBuilds.push(depPath)
    }
  }

  // waiting till package requests are finished
  const waitTillAllFetchingsFinish = async () =>
    Promise.all(
      Object.values(resolvedPackagesByDepPath).map(async ({ fetching }) => {
        try {
          await fetching?.()
        } catch {}
      })
    )

  return {
    dependenciesByProjectId,
    dependenciesGraph,
    finishLockfileUpdates: promiseShare(
      finishLockfileUpdates(
        dependenciesGraph,
        pendingRequiresBuilds,
        newLockfile
      )
    ),
    outdatedDependencies,
    linkedDependenciesByProjectId,
    newLockfile,
    peerDependencyIssuesByProjects,
    waitTillAllFetchingsFinish,
    wantedToBeSkippedPackageIds,
  }
}

function verifyPatches({
  patchedDependencies,
  appliedPatches,
  allowNonAppliedPatches,
}: {
  patchedDependencies: string[]
  appliedPatches: Set<string>
  allowNonAppliedPatches: boolean
}): void {
  const nonAppliedPatches: string[] = patchedDependencies.filter(
    (patchKey) => !appliedPatches.has(patchKey)
  )
  if (!nonAppliedPatches.length) return
  const message = `The following patches were not applied: ${nonAppliedPatches.join(', ')}`
  if (allowNonAppliedPatches) {
    globalWarn(message)
    return
  }
  throw new PnpmError('PATCH_NOT_APPLIED', message, {
    hint: 'Either remove them from "patchedDependencies" or update them to match packages in your dependencies.',
  })
}

async function finishLockfileUpdates(
  dependenciesGraph: DependenciesGraph,
  pendingRequiresBuilds: string[],
  newLockfile: Lockfile
) {
  return Promise.all(
    pendingRequiresBuilds.map(async (depPath: string) => {
      const depNode = dependenciesGraph[depPath]
      if (!depNode) return
      try {
        let requiresBuild: boolean
        if (depNode.optional) {
          // We assume that all optional dependencies have to be built.
          // Optional dependencies are not always downloaded, so there is no way to know whether they need to be built or not.
          requiresBuild = true
        } else {
          if (typeof depNode.fetching === 'undefined') {
            requiresBuild = false
          } else {
          // The npm team suggests to always read the package.json for deciding whether the package has lifecycle scripts
            const { files, bundledManifest: pkgJson } = await depNode.fetching()
            requiresBuild = Boolean(
              (pkgJson?.scripts != null &&
              (Boolean(pkgJson.scripts.preinstall) ||
                Boolean(pkgJson.scripts.install) ||
                Boolean(pkgJson.scripts.postinstall))) ||
              filesIncludeInstallScripts(files.filesIndex)
            )
          }
        }
        if (typeof depNode.requiresBuild === 'function') {
          depNode.requiresBuild.resolve(requiresBuild)
        }

        // TODO: try to cover with unit test the case when entry is no longer available in lockfile
        // It is an edge that probably happens if the entry is removed during lockfile prune
        if (requiresBuild && newLockfile.packages?.[depPath]) {
          newLockfile.packages[depPath].requiresBuild = true
        }
    } catch (err: any) { // eslint-disable-line
        if (typeof depNode.requiresBuild === 'function') {
          depNode.requiresBuild.reject(err)
        }
      }
    })
  )
}

function addDirectDependenciesToLockfile(
  newManifest: ProjectManifest,
  projectSnapshot: ProjectSnapshot,
  linkedPackages: Array<{ alias: string }>,
  directDependencies: ResolvedDirectDependency[],
  registries: Registries,
  excludeLinksFromLockfile?: boolean
): ProjectSnapshot {
  const newProjectSnapshot: ProjectSnapshot &
    Required<
      Pick<
        ProjectSnapshot,
        'dependencies' | 'devDependencies' | 'optionalDependencies'
      >
    > = {
      dependencies: {},
      devDependencies: {},
      optionalDependencies: {},
      specifiers: {},
    }

  if (newManifest.publishConfig?.directory) {
    newProjectSnapshot.publishDirectory = newManifest.publishConfig.directory
  }

  linkedPackages.forEach((linkedPkg) => {
    newProjectSnapshot.specifiers[linkedPkg.alias] = getSpecFromPackageManifest(
      newManifest,
      linkedPkg.alias
    )
  })

  const directDependenciesByAlias = directDependencies.reduce(
    (acc, directDependency) => {
      acc[directDependency.alias] = directDependency
      return acc
    },
    {} as Record<string, ResolvedDirectDependency>
  )

  const allDeps = Array.from(
    new Set(Object.keys(getAllDependenciesFromManifest(newManifest)))
  )

  for (const alias of allDeps) {
    const dep = directDependenciesByAlias[alias]
    const spec = dep && getSpecFromPackageManifest(newManifest, dep.alias)
    if (
      dep &&
      (!excludeLinksFromLockfile ||
        !(dep as LinkedDependency).isLinkedDependency ||
        spec.startsWith('workspace:'))
    ) {
      const ref = depPathToRef(dep.pkgId, {
        alias: dep.alias,
        realName: dep.name,
        registries,
        resolution: dep.resolution,
      })
      if (dep.dev) {
        newProjectSnapshot.devDependencies[dep.alias] = ref
      } else if (dep.optional) {
        newProjectSnapshot.optionalDependencies[dep.alias] = ref
      } else {
        newProjectSnapshot.dependencies[dep.alias] = ref
      }
      newProjectSnapshot.specifiers[dep.alias] = spec
    } else if (projectSnapshot.specifiers[alias]) {
      newProjectSnapshot.specifiers[alias] = projectSnapshot.specifiers[alias]
      if (projectSnapshot.dependencies?.[alias]) {
        newProjectSnapshot.dependencies[alias] =
          projectSnapshot.dependencies[alias]
      } else if (projectSnapshot.optionalDependencies?.[alias]) {
        newProjectSnapshot.optionalDependencies[alias] =
          projectSnapshot.optionalDependencies[alias]
      } else if (projectSnapshot.devDependencies?.[alias]) {
        newProjectSnapshot.devDependencies[alias] =
          projectSnapshot.devDependencies[alias]
      }
    }
  }

  alignDependencyTypes(newManifest, newProjectSnapshot)

  return newProjectSnapshot
}

function alignDependencyTypes(
  manifest: ProjectManifest,
  projectSnapshot: ProjectSnapshot
) {
  const depTypesOfAliases = getAliasToDependencyTypeMap(manifest)

  // Aligning the dependency types in pnpm-lock.yaml
  for (const depType of DEPENDENCIES_FIELDS) {
    if (projectSnapshot[depType] == null) continue
    for (const [alias, ref] of Object.entries(projectSnapshot[depType] ?? {})) {
      if (depType === depTypesOfAliases[alias] || !depTypesOfAliases[alias]) {
        continue
      }

      const ps = projectSnapshot[depTypesOfAliases[alias]] ?? {}

      projectSnapshot[depTypesOfAliases[alias]] = ps

      ps[alias] = ref

      delete projectSnapshot[depType]?.[alias]
    }
  }
}

function getAliasToDependencyTypeMap(
  manifest: ProjectManifest
): Record<string, DependenciesField> {
  const depTypesOfAliases: Record<string, DependenciesField> = {}
  for (const depType of DEPENDENCIES_FIELDS) {
    if (manifest[depType] == null) continue
    for (const alias of Object.keys(manifest[depType] ?? {})) {
      if (!depTypesOfAliases[alias]) {
        depTypesOfAliases[alias] = depType
      }
    }
  }
  return depTypesOfAliases
}

async function getTopParents(pkgAliases: string[], modulesDir: string) {
  const pkgs = await Promise.all(
    pkgAliases
      .map((alias) => path.join(modulesDir, alias))
      .map(safeReadPackageJsonFromDir)
  )
  return zipWith(
    (manifest, alias) => {
      if (!manifest) return null
      return {
        alias,
        name: manifest.name,
        version: manifest.version,
      }
    },
    pkgs,
    pkgAliases
  ).filter(Boolean) as DependencyManifest[]
}
