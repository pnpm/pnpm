import path from 'node:path'

import * as dp from '@pnpm/deps.path'
import { getPeerSatisfactionEdgesToSkip } from '@pnpm/lockfile.peer-edges'
import type {
  LockfileObject,
  PackageSnapshots,
  ProjectSnapshot,
} from '@pnpm/lockfile.types'
import type {
  DependenciesField,
  DepPath,
  PnpmSettings,
  ProjectId,
  ProjectManifest,
} from '@pnpm/types'
import normalizePath from 'normalize-path'
import { pick } from 'ramda'

import {
  type ConvertOptions,
  convertPackageSnapshot,
  convertProjectSnapshotToPackageSnapshot,
  convertResolvedDependencies,
  createFileUrlDepPath,
  type DeployAllProjects,
  resolveLinkOrFile,
} from './convertDeploySnapshots.js'
import {
  bindSingletonPeers,
  filterDeployPackageSnapshots,
  type LinkedWorkspaceProject,
} from './deployPackageGraph.js'

const DEPENDENCIES_FIELD = ['dependencies', 'devDependencies', 'optionalDependencies'] as const satisfies DependenciesField[]

export interface CreateDeployFilesOptions {
  allProjects: DeployAllProjects
  deployDir: string
  include: { [dependenciesField in DependenciesField]: boolean }
  lockfile: LockfileObject
  lockfileDir: string
  patchedDependencies?: PnpmSettings['patchedDependencies']
  selectedProjectManifest: ProjectManifest
  projectId: ProjectId
  resolvePeersFromWorkspaceRoot?: boolean
  rootProjectManifestDir: string
  allowBuilds?: Record<string, boolean | string>
}

export interface DeployWorkspaceManifest {
  allowBuilds?: Record<string, boolean | string>
  autoInstallPeers: boolean
  dedupePeers: boolean
  excludeLinksFromLockfile: boolean
  ignoredOptionalDependencies: string[]
  injectWorkspacePackages: false
  packages: ['.']
  patchedDependencies?: Record<string, string>
  peersSuffixMaxLength: number
  virtualStoreDir?: string
  virtualStoreType: 'project'
}

export interface DeployFiles {
  lockfile: LockfileObject
  manifest: ProjectManifest
  workspaceManifest: DeployWorkspaceManifest
}

export function createDeployFiles (opts: CreateDeployFilesOptions): DeployFiles {
  const deployedProjectRealPath = path.resolve(opts.lockfileDir, opts.projectId)
  const { peerSatisfactionEdges, targetPackageSnapshots } = convertLockfilePackages(opts, deployedProjectRealPath)
  const linkedWorkspaceProjects = convertWorkspaceImporters(opts, { deployedProjectRealPath, targetPackageSnapshots })
  const targetSnapshot = createDeployImporterSnapshot(opts, deployedProjectRealPath)

  const deployPackageSnapshots = filterDeployPackageSnapshots(
    targetSnapshot,
    targetPackageSnapshots,
    { include: opts.include, peerSatisfactionEdges }
  )
  bindSingletonPeers(targetSnapshot, deployPackageSnapshots, linkedWorkspaceProjects)

  const workspaceManifest = createDeployWorkspaceManifest(opts.lockfile)
  const result: DeployFiles = {
    lockfile: createDeployLockfile(opts.lockfile, { deployPackageSnapshots, targetSnapshot }),
    manifest: createDeployManifest(opts.selectedProjectManifest, targetSnapshot),
    workspaceManifest,
  }

  if (opts.lockfile.patchedDependencies && opts.patchedDependencies) {
    result.lockfile.patchedDependencies = { ...opts.lockfile.patchedDependencies }
    workspaceManifest.patchedDependencies = relativizePatchPaths(opts.patchedDependencies, opts.deployDir)
  }

  if (opts.allowBuilds) {
    workspaceManifest.allowBuilds = opts.allowBuilds
  }

  return result
}

function convertLockfilePackages (
  opts: CreateDeployFilesOptions,
  deployedProjectRealPath: string
): { peerSatisfactionEdges: Map<DepPath, ReadonlySet<string>>, targetPackageSnapshots: PackageSnapshots } {
  // Classified on the workspace lockfile: the deploy lockfile keeps only the
  // deployed project's included dependencies, so it no longer shows which
  // importers list a peer.
  const sourcePeerSatisfactionEdges = getPeerSatisfactionEdgesToSkip(opts.lockfile, {
    include: opts.include,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
  })
  const peerSatisfactionEdges = new Map<DepPath, ReadonlySet<string>>()
  const targetPackageSnapshots: PackageSnapshots = {}
  const convertOptions: ConvertOptions = {
    allProjects: opts.allProjects,
    deployDir: opts.deployDir,
    deployedProjectRealPath,
    lockfileDir: opts.lockfileDir,
    projectRootDirRealPath: opts.rootProjectManifestDir,
  }
  for (const name in opts.lockfile.packages) {
    const inputDepPath = name as DepPath
    const resolveResult = resolveLinkOrFile(inputDepPath, convertOptions)
    const outputDepPath = resolveResult
      ? createFileUrlDepPath(resolveResult, opts.allProjects)
      : inputDepPath
    const skippedAliases = sourcePeerSatisfactionEdges?.get(inputDepPath)
    if (skippedAliases != null) peerSatisfactionEdges.set(outputDepPath, skippedAliases)
    targetPackageSnapshots[outputDepPath] = convertPackageSnapshot(opts.lockfile.packages[inputDepPath], convertOptions)
  }
  return { peerSatisfactionEdges, targetPackageSnapshots }
}

function convertWorkspaceImporters (
  opts: CreateDeployFilesOptions,
  ctx: { deployedProjectRealPath: string, targetPackageSnapshots: PackageSnapshots }
): Map<DepPath, LinkedWorkspaceProject> {
  const peerBearingProjects = indexPeerBearingProjects(opts.allProjects)
  const linkedWorkspaceProjects = new Map<DepPath, LinkedWorkspaceProject>()
  const injectedWorkspace = opts.lockfile.settings?.injectWorkspacePackages === true
  for (const importerPath in opts.lockfile.importers) {
    if (importerPath === opts.projectId) continue
    const projectSnapshot = opts.lockfile.importers[importerPath as ProjectId]
    const projectRootDirRealPath = path.resolve(opts.lockfileDir, importerPath)
    const convertOptions: ConvertOptions = {
      allProjects: opts.allProjects,
      deployDir: opts.deployDir,
      lockfileDir: opts.lockfileDir,
      deployedProjectRealPath: ctx.deployedProjectRealPath,
      projectRootDirRealPath,
    }
    const packageSnapshot = convertProjectSnapshotToPackageSnapshot(projectSnapshot, convertOptions)
    const depPath = createFileUrlDepPath({ resolvedPath: projectRootDirRealPath }, opts.allProjects)
    ctx.targetPackageSnapshots[depPath] = packageSnapshot
    const manifest = peerBearingProjects.get(projectRootDirRealPath)
    if (manifest == null) continue
    linkedWorkspaceProjects.set(depPath, {
      manifest,
      dedupedPeerResolutions: injectedWorkspace
        ? convertResolvedDependencies(
          pick(Object.keys(manifest.peerDependencies ?? {}), projectSnapshot.devDependencies ?? {}),
          convertOptions
        )
        : undefined,
    })
  }
  return linkedWorkspaceProjects
}

/**
 * Indexed under both spellings of each project's directory, so the importer
 * loop costs one lookup per importer rather than a scan of every project: the
 * importer path is resolved lexically, while a project directory reached
 * through a symlink has a different real path.
 */
function indexPeerBearingProjects (allProjects: DeployAllProjects): Map<string, ProjectManifest> {
  const peerBearingProjects = new Map<string, ProjectManifest>()
  for (const project of allProjects) {
    if (project.manifest.peerDependencies == null) continue
    peerBearingProjects.set(project.rootDir, project.manifest)
    peerBearingProjects.set(project.rootDirRealPath, project.manifest)
  }
  return peerBearingProjects
}

function createDeployImporterSnapshot (opts: CreateDeployFilesOptions, deployedProjectRealPath: string): ProjectSnapshot {
  const inputSnapshot = opts.lockfile.importers[opts.projectId]
  const targetSnapshot: ProjectSnapshot = {
    ...inputSnapshot,
    specifiers: {},
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
  }
  const directDependencyNames = dependencyNames(opts.selectedProjectManifest)
  const peerOnlyDependencies = new Set(
    Object.keys(opts.selectedProjectManifest.peerDependencies ?? {}).filter(name => !directDependencyNames.has(name))
  )
  for (const field of DEPENDENCIES_FIELD) {
    // An excluded group's direct dependencies are left out of both the
    // deployed manifest and the deployed importer, because the graph filter
    // drops the packages they would point at. A runtime reference is
    // the exception: the engines field that generates it survives in the
    // deployed manifest and regenerates the edge on every read, so the
    // importer keeps it. The deploy install skips it with the rest of its
    // excluded group.
    const inputDependencies = inputSnapshot[field] ?? {}
    for (const name in inputDependencies) {
      const version = inputDependencies[name]
      if (!opts.include[field] && !peerOnlyDependencies.has(name) && !version.startsWith('runtime:')) continue
      addDeployImporterDependency(targetSnapshot, { deployedProjectRealPath, field, name, opts, version })
    }
  }
  return targetSnapshot
}

function addDeployImporterDependency (
  targetSnapshot: ProjectSnapshot,
  dependency: {
    deployedProjectRealPath: string
    field: DependenciesField
    name: string
    opts: CreateDeployFilesOptions
    version: string
  }
): void {
  const { deployedProjectRealPath, field, name, opts, version } = dependency
  const targetDependencies = targetSnapshot[field] ?? {}
  const targetSpecifiers = targetSnapshot.specifiers
  const resolveResult = resolveLinkOrFile(version, {
    lockfileDir: opts.lockfileDir,
    projectRootDirRealPath: path.resolve(opts.lockfileDir, opts.projectId),
  })

  if (!resolveResult) {
    targetDependencies[name] = version
    targetSpecifiers[name] = deployDependencySpecifier(name, version)
    return
  }

  resolveResult.packageName ??= name
  targetSpecifiers[name] = targetDependencies[name] =
    resolveResult.resolvedPath === deployedProjectRealPath ? 'link:.' : createFileUrlDepPath(resolveResult, opts.allProjects)
}

function createDeployWorkspaceManifest (lockfile: LockfileObject): DeployWorkspaceManifest {
  return {
    autoInstallPeers: lockfile.settings?.autoInstallPeers ?? true,
    dedupePeers: lockfile.settings?.dedupePeers ?? false,
    excludeLinksFromLockfile: lockfile.settings?.excludeLinksFromLockfile ?? false,
    ignoredOptionalDependencies: lockfile.ignoredOptionalDependencies ?? [],
    injectWorkspacePackages: false,
    packages: ['.'],
    peersSuffixMaxLength: lockfile.settings?.peersSuffixMaxLength ?? 1000,
    virtualStoreType: 'project',
  }
}

function createDeployLockfile (
  lockfile: LockfileObject,
  deployed: { deployPackageSnapshots: PackageSnapshots, targetSnapshot: ProjectSnapshot }
): LockfileObject {
  return {
    ...lockfile,
    // The deployed manifest contains concrete versions, and catalogs are not copied to the target.
    catalogs: undefined,
    patchedDependencies: undefined,
    overrides: undefined, // the effects of the overrides should already be part of the package snapshots
    packageExtensionsChecksum: undefined, // the effects of the package extensions should already be part of the package snapshots
    pnpmfileChecksum: undefined, // the effects of the pnpmfile should already be part of the package snapshots
    settings: {
      ...lockfile.settings,
      injectWorkspacePackages: undefined, // the effects of injecting workspace packages should already be part of the lockfile
    },
    importers: {
      ['.' as ProjectId]: deployed.targetSnapshot,
    },
    packages: deployed.deployPackageSnapshots,
  }
}

function createDeployManifest (selectedProjectManifest: ProjectManifest, targetSnapshot: ProjectSnapshot): ProjectManifest {
  return omitPeersOfExcludedDependencies({
    ...selectedProjectManifest,
    dependencies: pick(Object.keys(targetSnapshot.dependencies ?? {}), targetSnapshot.specifiers),
    devDependencies: pick(Object.keys(targetSnapshot.devDependencies ?? {}), targetSnapshot.specifiers),
    optionalDependencies: pick(Object.keys(targetSnapshot.optionalDependencies ?? {}), targetSnapshot.specifiers),
  }, selectedProjectManifest, targetSnapshot)
}

function relativizePatchPaths (patchedDependencies: Record<string, string>, deployDir: string): Record<string, string> {
  const deployManifestPatchedDeps: Record<string, string> = {}
  for (const name in patchedDependencies) {
    const absolutePath = patchedDependencies[name]
    const relativePath = normalizePath(path.relative(deployDir, absolutePath))
    deployManifestPatchedDeps[name] = relativePath
  }
  return deployManifestPatchedDeps
}

function deployDependencySpecifier (alias: string, reference: string): string {
  const depPath = dp.refToRelative(reference, alias)
  if (depPath == null) return reference
  const { name, version, registryName } = dp.parse(depPath)
  if (version == null || registryName != null) return reference
  return name === alias ? version : `npm:${name}@${version}`
}

function omitPeersOfExcludedDependencies (
  manifest: ProjectManifest,
  inputManifest: ProjectManifest,
  targetSnapshot: ProjectSnapshot
): ProjectManifest {
  const includedDependencies = dependencyNames(targetSnapshot)
  const excludedDependencies = new Set(
    Array.from(dependencyNames(inputManifest)).filter(name => !includedDependencies.has(name))
  )
  if (excludedDependencies.size === 0) return manifest

  return {
    ...manifest,
    peerDependencies: omitKeys(manifest.peerDependencies, excludedDependencies),
    peerDependenciesMeta: omitKeys(manifest.peerDependenciesMeta, excludedDependencies),
  }
}

function dependencyNames (source: ProjectManifest | ProjectSnapshot): Set<string> {
  return new Set(DEPENDENCIES_FIELD.flatMap(field => Object.keys(source[field] ?? {})))
}

function omitKeys<Value> (record: Record<string, Value> | undefined, keys: Set<string>): Record<string, Value> | undefined {
  if (record == null) return undefined
  return Object.fromEntries(Object.entries(record).filter(([key]) => !keys.has(key)))
}
