import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import {
  packageManifestLogger,
} from '@pnpm/core-loggers'
import { globalWarn } from '@pnpm/logger'
import {
  Lockfile,
  ProjectSnapshot,
} from '@pnpm/lockfile-types'
import {
  getAllDependenciesFromManifest,
  getSpecFromPackageManifest,
  PinnedVersion,
} from '@pnpm/manifest-utils'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import {
  DEPENDENCIES_FIELDS,
  DependencyManifest,
  ProjectManifest,
  Registries,
} from '@pnpm/types'
import promiseShare from 'promise-share'
import difference from 'ramda/src/difference'
import getWantedDependencies, { WantedDependency } from './getWantedDependencies'
import depPathToRef from './depPathToRef'
import resolveDependencyTree, {
  Importer,
  LinkedDependency,
  ResolveDependenciesOptions,
  ResolvedDirectDependency,
  ResolvedPackage,
} from './resolveDependencyTree'
import resolvePeers, {
  GenericDependenciesGraph,
  GenericDependenciesGraphNode,
} from './resolvePeers'
import toResolveImporter from './toResolveImporter'
import updateLockfile from './updateLockfile'
import updateProjectManifest from './updateProjectManifest'

export type DependenciesGraph = GenericDependenciesGraph<ResolvedPackage>

export type DependenciesGraphNode = GenericDependenciesGraphNode & ResolvedPackage

export {
  getWantedDependencies,
  LinkedDependency,
  ResolvedPackage,
  PinnedVersion,
  WantedDependency,
}

interface ProjectToLink {
  binsDir: string
  directNodeIdsByAlias: { [alias: string]: string }
  id: string
  linkedDependencies: LinkedDependency[]
  manifest: ProjectManifest
  modulesDir: string
  rootDir: string
  topParents: Array<{ name: string, version: string }>
}

export type ImporterToResolve = Importer<{
  isNew?: boolean
  nodeExecPath?: string
  pinnedVersion?: PinnedVersion
  raw: string
  updateSpec?: boolean
}>
& {
  binsDir: string
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  updatePackageManifest: boolean
}

export async function resolveDependencies (
  importers: ImporterToResolve[],
  opts: ResolveDependenciesOptions & {
    defaultUpdateDepth: number
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
    updateAll: Boolean(opts.updateMatching),
    virtualStoreDir: opts.virtualStoreDir,
    workspacePackages: opts.workspacePackages,
  })
  const projectsToResolve = await Promise.all(importers.map(async (project) => _toResolveImporter(project)))
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
    (opts.forceFullResolution || !opts.wantedLockfile.packages?.length) &&
    Object.keys(opts.wantedLockfile.importers).length === importers.length
  ) {
    verifyPatches({
      patchedDependencies: Object.keys(opts.patchedDependencies),
      appliedPatches,
      allowNonAppliedPatches: opts.allowNonAppliedPatches,
    })
  }

  const linkedDependenciesByProjectId: Record<string, LinkedDependency[]> = {}
  const projectsToLink = await Promise.all<ProjectToLink>(projectsToResolve.map(async (project, index) => {
    const resolvedImporter = resolvedImporters[project.id]
    linkedDependenciesByProjectId[project.id] = resolvedImporter.linkedDependencies
    let updatedManifest: ProjectManifest | undefined = project.manifest
    let updatedOriginalManifest: ProjectManifest | undefined = project.originalManifest
    if (project.updatePackageManifest) {
      const manifests = await updateProjectManifest(project, {
        directDependencies: resolvedImporter.directDependencies,
        preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
        saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      })
      updatedManifest = manifests[0]
      updatedOriginalManifest = manifests[1]
    } else {
      packageManifestLogger.debug({
        prefix: project.rootDir,
        updated: project.manifest,
      })
    }

    if (updatedManifest != null) {
      if (opts.autoInstallPeers) {
        if (updatedManifest.peerDependencies) {
          const allDeps = getAllDependenciesFromManifest(updatedManifest)
          for (const [peerName, peerRange] of Object.entries(updatedManifest.peerDependencies)) {
            if (allDeps[peerName]) continue
            updatedManifest.dependencies ??= {}
            updatedManifest.dependencies[peerName] = peerRange
          }
        }
      }
      const projectSnapshot = opts.wantedLockfile.importers[project.id]
      opts.wantedLockfile.importers[project.id] = addDirectDependenciesToLockfile(
        updatedManifest,
        projectSnapshot,
        resolvedImporter.linkedDependencies,
        resolvedImporter.directDependencies,
        opts.registries,
        opts.autoInstallPeers
      )
    }

    const topParents: Array<{ name: string, version: string, linkedDir?: string }> = project.manifest
      ? await getTopParents(
        difference(
          Object.keys(getAllDependenciesFromManifest(project.manifest)),
          resolvedImporter.directDependencies
            .filter((dep, index) => project.wantedDependencies[index]?.isNew === true)
            .map(({ alias }) => alias) || []
        ),
        project.modulesDir
      )
      : []
    resolvedImporter.linkedDependencies.forEach((linkedDependency) => {
      topParents.push({
        name: linkedDependency.alias,
        version: linkedDependency.version,
        linkedDir: `link:${path.relative(opts.lockfileDir, linkedDependency.resolution.directory)}`,
      })
    })

    const manifest = updatedOriginalManifest ?? project.originalManifest ?? project.manifest
    importers[index].manifest = manifest
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
  }))

  const {
    dependenciesGraph,
    dependenciesByProjectId,
    peerDependencyIssuesByProjects,
  } = resolvePeers({
    dependenciesTree,
    lockfileDir: opts.lockfileDir,
    projects: projectsToLink,
    virtualStoreDir: opts.virtualStoreDir,
  })

  for (const { id, manifest } of projectsToLink) {
    for (const [alias, depPath] of Object.entries(dependenciesByProjectId[id])) {
      const projectSnapshot = opts.wantedLockfile.importers[id]
      if (manifest.dependenciesMeta != null) {
        projectSnapshot.dependenciesMeta = manifest.dependenciesMeta
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
        (opts.wantedLockfile.packages?.[depPath] == null) ||
        pkg.requiresBuild === true
      ) continue
      pendingRequiresBuilds.push(depPath)
    }
  }

  // waiting till package requests are finished
  const waitTillAllFetchingsFinish = async () => Promise.all(Object.values(resolvedPackagesByDepPath).map(async ({ finishing }) => finishing?.()))

  return {
    dependenciesByProjectId,
    dependenciesGraph,
    finishLockfileUpdates: promiseShare(finishLockfileUpdates(dependenciesGraph, pendingRequiresBuilds, newLockfile)),
    outdatedDependencies,
    linkedDependenciesByProjectId,
    newLockfile,
    peerDependencyIssuesByProjects,
    waitTillAllFetchingsFinish,
    wantedToBeSkippedPackageIds,
  }
}

function verifyPatches (
  {
    patchedDependencies,
    appliedPatches,
    allowNonAppliedPatches,
  }: {
    patchedDependencies: string[]
    appliedPatches: Set<string>
    allowNonAppliedPatches: boolean
  }
): void {
  const nonAppliedPatches: string[] = patchedDependencies.filter((patchKey) => !appliedPatches.has(patchKey))
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

async function finishLockfileUpdates (
  dependenciesGraph: DependenciesGraph,
  pendingRequiresBuilds: string[],
  newLockfile: Lockfile
) {
  return Promise.all(pendingRequiresBuilds.map(async (depPath) => {
    const depNode = dependenciesGraph[depPath]
    let requiresBuild!: boolean
    if (depNode.optional) {
      // We assume that all optional dependencies have to be built.
      // Optional dependencies are not always downloaded, so there is no way to know whether they need to be built or not.
      requiresBuild = true
    } else if (depNode.fetchingBundledManifest != null) {
      const filesResponse = await depNode.fetchingFiles()
      // The npm team suggests to always read the package.json for deciding whether the package has lifecycle scripts
      const pkgJson = await depNode.fetchingBundledManifest()
      requiresBuild = Boolean(
        pkgJson?.scripts != null && (
          Boolean(pkgJson.scripts.preinstall) ||
          Boolean(pkgJson.scripts.install) ||
          Boolean(pkgJson.scripts.postinstall)
        ) ||
        filesResponse.filesIndex['binding.gyp'] ||
          Object.keys(filesResponse.filesIndex).some((filename) => !(filename.match(/^[.]hooks[\\/]/) == null)) // TODO: optimize this
      )
    } else {
      // This should never ever happen
      throw new Error(`Cannot create ${WANTED_LOCKFILE} because raw manifest (aka package.json) wasn't fetched for "${depPath}"`)
    }
    if (typeof depNode.requiresBuild === 'function') {
      depNode.requiresBuild['resolve'](requiresBuild)
    }

    // TODO: try to cover with unit test the case when entry is no longer available in lockfile
    // It is an edge that probably happens if the entry is removed during lockfile prune
    if (requiresBuild && newLockfile.packages![depPath]) {
      newLockfile.packages![depPath].requiresBuild = true
    }
  }))
}

function addDirectDependenciesToLockfile (
  newManifest: ProjectManifest,
  projectSnapshot: ProjectSnapshot,
  linkedPackages: Array<{ alias: string }>,
  directDependencies: ResolvedDirectDependency[],
  registries: Registries,
  autoInstallPeers?: boolean
): ProjectSnapshot {
  const newProjectSnapshot: ProjectSnapshot & Required<Pick<ProjectSnapshot, 'dependencies' | 'devDependencies' | 'optionalDependencies'>> = {
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    specifiers: {},
  }

  if (newManifest.publishConfig?.directory) {
    newProjectSnapshot.publishDirectory = newManifest.publishConfig.directory
  }

  linkedPackages.forEach((linkedPkg) => {
    newProjectSnapshot.specifiers[linkedPkg.alias] = getSpecFromPackageManifest(newManifest, linkedPkg.alias)
  })

  const directDependenciesByAlias = directDependencies.reduce((acc, directDependency) => {
    acc[directDependency.alias] = directDependency
    return acc
  }, {})

  const allDeps = Array.from(new Set(Object.keys(getAllDependenciesFromManifest(newManifest))))

  for (const alias of allDeps) {
    if (directDependenciesByAlias[alias]) {
      const dep = directDependenciesByAlias[alias]
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
      newProjectSnapshot.specifiers[dep.alias] = getSpecFromPackageManifest(newManifest, dep.alias)
    } else if (projectSnapshot.specifiers[alias]) {
      newProjectSnapshot.specifiers[alias] = projectSnapshot.specifiers[alias]
      if (projectSnapshot.dependencies?.[alias]) {
        newProjectSnapshot.dependencies[alias] = projectSnapshot.dependencies[alias]
      } else if (projectSnapshot.optionalDependencies?.[alias]) {
        newProjectSnapshot.optionalDependencies[alias] = projectSnapshot.optionalDependencies[alias]
      } else if (projectSnapshot.devDependencies?.[alias]) {
        newProjectSnapshot.devDependencies[alias] = projectSnapshot.devDependencies[alias]
      }
    }
  }

  alignDependencyTypes(newManifest, newProjectSnapshot)

  return newProjectSnapshot
}

function alignDependencyTypes (manifest: ProjectManifest, projectSnapshot: ProjectSnapshot) {
  const depTypesOfAliases = getAliasToDependencyTypeMap(manifest)

  // Aligning the dependency types in pnpm-lock.yaml
  for (const depType of DEPENDENCIES_FIELDS) {
    if (projectSnapshot[depType] == null) continue
    for (const alias of Object.keys(projectSnapshot[depType] ?? {})) {
      if (depType === depTypesOfAliases[alias] || !depTypesOfAliases[alias]) continue
      projectSnapshot[depTypesOfAliases[alias]][alias] = projectSnapshot[depType]![alias]
      delete projectSnapshot[depType]![alias]
    }
  }
}

function getAliasToDependencyTypeMap (manifest: ProjectManifest) {
  const depTypesOfAliases = {}
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

async function getTopParents (pkgNames: string[], modules: string) {
  const pkgs = await Promise.all(
    pkgNames.map((pkgName) => path.join(modules, pkgName)).map(safeReadPackageJsonFromDir)
  )
  return (
    pkgs
      .filter(Boolean) as DependencyManifest[]
  )
    .map(({ name, version }: DependencyManifest) => ({ name, version }))
}
