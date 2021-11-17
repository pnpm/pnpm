import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  packageManifestLogger,
} from '@pnpm/core-loggers'
import {
  Lockfile,
  ProjectSnapshot,
} from '@pnpm/lockfile-types'
import {
  getAllDependenciesFromManifest,
  getSpecFromPackageManifest,
  PinnedVersion,
} from '@pnpm/manifest-utils'
import { safeReadPackageFromDir as safeReadPkgFromDir } from '@pnpm/read-package-json'
import {
  DEPENDENCIES_FIELDS,
  DependencyManifest,
  ProjectManifest,
  Registries,
} from '@pnpm/types'
import difference from 'ramda/src/difference'
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
import updateLockfile from './updateLockfile'
import updateProjectManifest from './updateProjectManifest'

export type DependenciesGraph = GenericDependenciesGraph<ResolvedPackage>

export type DependenciesGraphNode = GenericDependenciesGraphNode & ResolvedPackage

export {
  LinkedDependency,
  ResolvedPackage,
}

interface ProjectToLink {
  binsDir: string
  directNodeIdsByAlias: {[alias: string]: string}
  id: string
  linkedDependencies: LinkedDependency[]
  manifest: ProjectManifest
  modulesDir: string
  rootDir: string
  topParents: Array<{name: string, version: string}>
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

export default async function (
  importers: ImporterToResolve[],
  opts: ResolveDependenciesOptions & {
    preserveWorkspaceProtocol: boolean
    saveWorkspaceProtocol: boolean
    strictPeerDependencies: boolean
  }
) {
  const {
    dependenciesTree,
    outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByDepPath,
    wantedToBeSkippedPackageIds,
  } = await resolveDependencyTree(importers, opts)

  const linkedDependenciesByProjectId: Record<string, LinkedDependency[]> = {}
  const projectsToLink = await Promise.all<ProjectToLink>(importers.map(async (project, index) => {
    const resolvedImporter = resolvedImporters[project.id]
    linkedDependenciesByProjectId[project.id] = resolvedImporter.linkedDependencies
    let updatedManifest: ProjectManifest | undefined = project.manifest
    let updatedOriginalManifest: ProjectManifest | undefined = project.originalManifest
    if (project.updatePackageManifest) {
      const manifests = await updateProjectManifest(importers[index], {
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
      const projectSnapshot = opts.wantedLockfile.importers[project.id]
      opts.wantedLockfile.importers[project.id] = addDirectDependenciesToLockfile(
        updatedManifest,
        projectSnapshot,
        resolvedImporter.linkedDependencies,
        resolvedImporter.directDependencies,
        opts.registries
      )
    }

    const topParents = project.manifest
      ? await getTopParents(
        difference(
          Object.keys(getAllDependenciesFromManifest(project.manifest)),
          resolvedImporter.directDependencies
            .filter((dep, index) => project.wantedDependencies[index].isNew === true)
            .map(({ alias }) => alias) || []
        ),
        project.modulesDir
      )
      : []

    project.manifest = updatedOriginalManifest ?? project.originalManifest ?? project.manifest
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
  } = resolvePeers({
    dependenciesTree,
    lockfileDir: opts.lockfileDir,
    projects: projectsToLink,
    strictPeerDependencies: opts.strictPeerDependencies,
    virtualStoreDir: opts.virtualStoreDir,
  })

  for (const { id, manifest } of projectsToLink) {
    for (const [alias, depPath] of Object.entries(dependenciesByProjectId[id])) {
      const projectSnapshot = opts.wantedLockfile.importers[id]
      if (manifest.dependenciesMeta != null) {
        projectSnapshot.dependenciesMeta = manifest.dependenciesMeta
      }

      const depNode = dependenciesGraph[depPath]
      if (depNode.isPure) continue

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

  const { newLockfile, pendingRequiresBuilds } = updateLockfile(dependenciesGraph, opts.wantedLockfile, opts.virtualStoreDir, opts.registries) // eslint-disable-line:prefer-const

  if (opts.forceFullResolution && opts.wantedLockfile != null) {
    for (const [depPath, pkg] of Object.entries(dependenciesGraph)) {
      if (
        // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
        opts.neverBuiltDependencies?.has(pkg.name) ||
        (opts.onlyBuiltDependencies === false || opts.onlyBuiltDependencies.has(pkg.name)) ||
        opts.wantedLockfile.packages?.[depPath] == null ||
        pkg.requiresBuild
      ) continue
      pendingRequiresBuilds.push(depPath)
    }
  }

  // waiting till package requests are finished
  const waitTillAllFetchingsFinish = async () => Promise.all(Object.values(resolvedPackagesByDepPath).map(async ({ finishing }) => finishing?.()))

  return {
    dependenciesByProjectId,
    dependenciesGraph,
    finishLockfileUpdates: finishLockfileUpdates.bind(null, dependenciesGraph, pendingRequiresBuilds, newLockfile),
    outdatedDependencies,
    linkedDependenciesByProjectId,
    newLockfile,
    waitTillAllFetchingsFinish,
    wantedToBeSkippedPackageIds,
  }
}

async function finishLockfileUpdates (
  dependenciesGraph: DependenciesGraph,
  pendingRequiresBuilds: string[],
  newLockfile: Lockfile
) {
  return Promise.all(pendingRequiresBuilds.map(async (depPath) => {
    const depNode = dependenciesGraph[depPath]
    if (depNode.optional) {
      // We assume that all optional dependencies have to be built.
      // Optional dependencies are not always downloaded, so there is no way to know whether they need to be built or not.
      depNode.requiresBuild = true
    } else if (depNode.fetchingBundledManifest != null) {
      const filesResponse = await depNode.fetchingFiles()
      // The npm team suggests to always read the package.json for deciding whether the package has lifecycle scripts
      const pkgJson = await depNode.fetchingBundledManifest()
      depNode.requiresBuild = Boolean(
        pkgJson.scripts != null && (
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

    // TODO: try to cover with unit test the case when entry is no longer available in lockfile
    // It is an edge that probably happens if the entry is removed during lockfile prune
    if (depNode.requiresBuild && newLockfile.packages![depPath]) {
      newLockfile.packages![depPath].requiresBuild = true
    }
  }))
}

function addDirectDependenciesToLockfile (
  newManifest: ProjectManifest,
  projectSnapshot: ProjectSnapshot,
  linkedPackages: Array<{alias: string}>,
  directDependencies: ResolvedDirectDependency[],
  registries: Registries
): ProjectSnapshot {
  const newProjectSnapshot = {
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    specifiers: {},
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
    pkgNames.map((pkgName) => path.join(modules, pkgName)).map(safeReadPkgFromDir)
  )
  return (
    pkgs
      .filter(Boolean) as DependencyManifest[]
  )
    .map(({ name, version }: DependencyManifest) => ({ name, version }))
}
