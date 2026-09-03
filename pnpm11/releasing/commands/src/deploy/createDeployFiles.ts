import path from 'node:path'
import url from 'node:url'

import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type {
  DirectoryResolution,
  LockfileObject,
  LockfileResolution,
  PackageSnapshot,
  PackageSnapshots,
  ProjectSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import type {
  DependenciesField,
  DepPath,
  PnpmSettings,
  Project,
  ProjectId,
  ProjectManifest,
} from '@pnpm/types'
import normalizePath from 'normalize-path'

const DEPENDENCIES_FIELD = ['dependencies', 'devDependencies', 'optionalDependencies'] as const satisfies DependenciesField[]

export interface CreateDeployFilesOptions {
  allProjects: Array<Pick<Project, 'manifest' | 'rootDir' | 'rootDirRealPath'>>
  deployDir: string
  include: { [dependenciesField in DependenciesField]: boolean }
  lockfile: LockfileObject
  lockfileDir: string
  patchedDependencies?: PnpmSettings['patchedDependencies']
  selectedProjectManifest: ProjectManifest
  projectId: ProjectId
  rootProjectManifestDir: string
  allowBuilds?: Record<string, boolean | string>
}

export interface DeployWorkspaceManifest {
  allowBuilds?: Record<string, boolean | string>
  patchedDependencies?: Record<string, string>
}

export interface DeployFiles {
  lockfile: LockfileObject
  manifest: ProjectManifest
  workspaceManifest?: DeployWorkspaceManifest
}

export function createDeployFiles ({
  allProjects,
  deployDir,
  include,
  lockfile,
  lockfileDir,
  patchedDependencies,
  selectedProjectManifest,
  projectId,
  rootProjectManifestDir,
  allowBuilds,
}: CreateDeployFilesOptions): DeployFiles {
  const deployedProjectRealPath = path.resolve(lockfileDir, projectId)
  const inputSnapshot = lockfile.importers[projectId]

  const targetSnapshot: ProjectSnapshot = {
    ...inputSnapshot,
    specifiers: {},
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
  }
  const directDependencyNames = dependencyNames(selectedProjectManifest)
  const peerOnlyDependencies = new Set(
    Object.keys(selectedProjectManifest.peerDependencies ?? {}).filter(name => !directDependencyNames.has(name))
  )

  const targetPackageSnapshots: PackageSnapshots = {}
  for (const name in lockfile.packages) {
    const inputDepPath = name as DepPath
    const inputSnapshot = lockfile.packages[inputDepPath]
    const resolveResult = resolveLinkOrFile(inputDepPath, {
      lockfileDir,
      projectRootDirRealPath: rootProjectManifestDir,
    })
    const outputDepPath = resolveResult
      ? createFileUrlDepPath(resolveResult, allProjects)
      : inputDepPath
    targetPackageSnapshots[outputDepPath] = convertPackageSnapshot(inputSnapshot, {
      allProjects,
      deployDir,
      deployedProjectRealPath,
      lockfileDir,
      projectRootDirRealPath: rootProjectManifestDir,
    })
  }

  // Indexed under both spellings of each project's directory, so the importer
  // loop below costs one lookup per importer rather than a scan of every
  // project: the importer path is resolved lexically, while a project directory
  // reached through a symlink has a different real path.
  const peerBearingProjects = new Map<string, ProjectManifest>()
  for (const project of allProjects) {
    if (project.manifest.peerDependencies == null) continue
    peerBearingProjects.set(project.rootDir, project.manifest)
    peerBearingProjects.set(project.rootDirRealPath, project.manifest)
  }

  const linkedWorkspaceProjects = new Map<DepPath, ProjectManifest>()
  for (const importerPath in lockfile.importers) {
    if (importerPath === projectId) continue
    const projectSnapshot = lockfile.importers[importerPath as ProjectId]
    const projectRootDirRealPath = path.resolve(lockfileDir, importerPath)
    const packageSnapshot = convertProjectSnapshotToPackageSnapshot(projectSnapshot, {
      allProjects,
      deployDir,
      lockfileDir,
      deployedProjectRealPath,
      projectRootDirRealPath,
    })
    const depPath = createFileUrlDepPath({ resolvedPath: projectRootDirRealPath }, allProjects)
    targetPackageSnapshots[depPath] = packageSnapshot
    const manifest = peerBearingProjects.get(projectRootDirRealPath)
    if (manifest != null) linkedWorkspaceProjects.set(depPath, manifest)
  }

  for (const field of DEPENDENCIES_FIELD) {
    // An excluded group's direct dependencies are left out of both the
    // deployed manifest and the deployed importer, because the graph filter
    // below drops the packages they would point at.
    const targetDependencies = targetSnapshot[field] ?? {}
    const targetSpecifiers = targetSnapshot.specifiers
    const inputDependencies = inputSnapshot[field] ?? {}
    for (const name in inputDependencies) {
      if (!include[field] && !peerOnlyDependencies.has(name)) continue
      const version = inputDependencies[name]
      const resolveResult = resolveLinkOrFile(version, {
        lockfileDir,
        projectRootDirRealPath: path.resolve(lockfileDir, projectId),
      })

      if (!resolveResult) {
        targetSpecifiers[name] = targetDependencies[name] = version
        continue
      }

      resolveResult.packageName ??= name
      targetSpecifiers[name] = targetDependencies[name] =
        resolveResult.resolvedPath === deployedProjectRealPath ? 'link:.' : createFileUrlDepPath(resolveResult, allProjects)
    }
  }

  const deployPackageSnapshots = filterDeployPackageSnapshots(
    targetSnapshot,
    targetPackageSnapshots,
    include
  )
  bindSingletonPeers(targetSnapshot, deployPackageSnapshots, linkedWorkspaceProjects)

  const result: DeployFiles = {
    lockfile: {
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
        ['.' as ProjectId]: targetSnapshot,
      },
      packages: deployPackageSnapshots,
    },
    manifest: omitPeersOfExcludedDependencies({
      ...selectedProjectManifest,
      dependencies: targetSnapshot.dependencies,
      devDependencies: targetSnapshot.devDependencies,
      optionalDependencies: targetSnapshot.optionalDependencies,
    }, selectedProjectManifest, targetSnapshot),
  }

  if (lockfile.patchedDependencies && patchedDependencies) {
    result.lockfile.patchedDependencies = { ...lockfile.patchedDependencies }
    const deployManifestPatchedDeps: Record<string, string> = {}
    for (const name in patchedDependencies) {
      const absolutePath = patchedDependencies[name]
      const relativePath = normalizePath(path.relative(deployDir, absolutePath))
      deployManifestPatchedDeps[name] = relativePath
    }
    result.workspaceManifest = {
      ...result.workspaceManifest,
      patchedDependencies: deployManifestPatchedDeps,
    }
  }

  if (allowBuilds) {
    result.workspaceManifest = {
      ...result.workspaceManifest,
      allowBuilds,
    }
  }

  return result
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

function omitKeys<T> (record: Record<string, T> | undefined, keys: Set<string>): Record<string, T> | undefined {
  if (record == null) return undefined
  return Object.fromEntries(Object.entries(record).filter(([key]) => !keys.has(key)))
}

/** Takes ownership of `packages`: the retained snapshots are edited in place. */
function filterDeployPackageSnapshots (
  importer: ProjectSnapshot,
  packages: PackageSnapshots,
  include: CreateDeployFilesOptions['include']
): PackageSnapshots {
  const queue: DepPath[] = []
  const enqueue = (dependencies: ResolvedDependencies | undefined) => {
    for (const [alias, reference] of Object.entries(dependencies ?? {})) {
      const depPath = dp.refToRelative(reference, alias)
      if (depPath != null && packages[depPath] != null) queue.push(depPath)
    }
  }

  enqueue(importer.dependencies)
  enqueue(importer.devDependencies)
  enqueue(importer.optionalDependencies)

  const reachable = new Set<DepPath>()
  let head = 0
  while (head < queue.length) {
    const depPath = queue[head++]!
    if (reachable.has(depPath)) continue
    reachable.add(depPath)

    const snapshot = packages[depPath]
    if (snapshot == null) continue
    enqueue(snapshot.dependencies)
    if (include.optionalDependencies) enqueue(snapshot.optionalDependencies)
  }

  return Object.fromEntries(
    Array.from(reachable, (depPath) => {
      const snapshot = packages[depPath]
      // A retained snapshot's optional edges point at packages this filter just dropped.
      if (!include.optionalDependencies) snapshot.optionalDependencies = undefined
      return [depPath, snapshot]
    })
  ) as PackageSnapshots
}

/**
 * Resolves the peer dependencies of linked workspace packages against the
 * deployed graph, editing the snapshots in `packages` in place.
 *
 * A linked workspace package has no package snapshot in the shared lockfile, so
 * the importer its deployed snapshot is synthesized from carries no peer
 * bindings and they cannot be recovered afterwards. A peer already bound by
 * either dependency map, or absent from the deployed graph entirely, is left
 * alone.
 *
 * @throws PnpmError DEPLOY_AMBIGUOUS_PEER when the deployed graph offers more
 * than one resolution for a peer, since choosing between them is precisely the
 * decision injecting the package would have made.
 */
function bindSingletonPeers (
  importer: ProjectSnapshot,
  packages: PackageSnapshots,
  linkedWorkspaceProjects: Map<DepPath, ProjectManifest>
): void {
  if (linkedWorkspaceProjects.size === 0) return

  // Keyed by the resolved dependency path rather than the reference that
  // spelled it, so an npm-aliased edge and a plain one that name the same
  // package count once.
  const references = new Map<string, Set<string>>()
  const collect = (dependencies: ResolvedDependencies | undefined) => {
    for (const [alias, reference] of Object.entries(dependencies ?? {})) {
      const depPath = dp.refToRelative(reference, alias)
      if (depPath == null) continue
      const { name } = dp.parse(depPath)
      if (name == null) continue
      let referencesOfName = references.get(name)
      if (referencesOfName == null) references.set(name, referencesOfName = new Set())
      referencesOfName.add(depPath.slice(name.length + 1))
    }
  }
  collect(importer.dependencies)
  collect(importer.devDependencies)
  collect(importer.optionalDependencies)
  for (const snapshot of Object.values(packages)) {
    collect(snapshot.dependencies)
    collect(snapshot.optionalDependencies)
  }

  for (const [depPath, manifest] of linkedWorkspaceProjects) {
    const snapshot = packages[depPath]
    if (snapshot == null) continue
    for (const peerName of Object.keys(manifest.peerDependencies ?? {})) {
      // Consult the manifest, not just the snapshot: the graph prune clears the
      // optional map before this runs, so a peer the package depends on
      // optionally is invisible in the snapshot under `--no-optional`, and
      // binding it there would resurrect a dependency the flag excluded.
      if (declaresDependency(manifest, peerName)) continue
      if (declaresDependency(snapshot, peerName)) continue
      const candidates = references.get(peerName)
      // A peer the deployed graph does not provide at all stays unresolved,
      // exactly as it is in the workspace this deploy was taken from.
      if (candidates == null) continue
      if (candidates.size > 1) {
        throw new PnpmError('DEPLOY_AMBIGUOUS_PEER', `Workspace package '${manifest.name ?? depPath}' declares a peer dependency on '${peerName}', which resolves to more than one version (${Array.from(candidates).sort().join(', ')}) in the deployed graph. Without "injectWorkspacePackages" there is no snapshot to bind it to.`, {
          hint: `Pin '${peerName}' to a single version with an "overrides" entry, set "injectWorkspacePackages" to true, or run "pnpm deploy" with the "--legacy" flag.`,
        })
      }
      snapshot.dependencies = { ...snapshot.dependencies, [peerName]: Array.from(candidates)[0] }
    }
  }
}

/**
 * Whether `source` binds `name` through one of its runtime dependency maps.
 *
 * Own keys only: a package may legitimately be named `constructor` or
 * `toString`, and a plain property read would find those on `Object.prototype`
 * and report a binding that does not exist.
 */
function declaresDependency (
  source: Pick<ProjectManifest, 'dependencies' | 'optionalDependencies'> | Pick<PackageSnapshot, 'dependencies' | 'optionalDependencies'>,
  name: string
): boolean {
  return (source.dependencies != null && Object.hasOwn(source.dependencies, name)) ||
    (source.optionalDependencies != null && Object.hasOwn(source.optionalDependencies, name))
}

interface ConvertOptions {
  allProjects: CreateDeployFilesOptions['allProjects']
  deployDir: string
  deployedProjectRealPath: string
  projectRootDirRealPath: string
  lockfileDir: string
}

function convertPackageSnapshot (inputSnapshot: PackageSnapshot, opts: ConvertOptions): PackageSnapshot {
  const inputResolution = inputSnapshot.resolution
  let outputResolution: LockfileResolution
  if ('integrity' in inputResolution) {
    outputResolution = inputResolution
  } else if ('tarball' in inputResolution && typeof inputResolution.tarball === 'string') {
    outputResolution = { ...inputResolution }
    if (inputResolution.tarball.startsWith('file:')) {
      const inputPath = inputResolution.tarball.slice('file:'.length)
      const resolvedPath = path.resolve(opts.lockfileDir, inputPath)
      const outputPath = normalizePath(path.relative(opts.deployDir, resolvedPath))
      outputResolution.tarball = `file:${outputPath}`
      if ('path' in inputResolution && typeof inputResolution.path === 'string') {
        outputResolution.path = outputPath
      }
    }
  } else if (inputResolution.type === 'directory') {
    const dirResolution = inputResolution as DirectoryResolution
    const resolvedPath = path.resolve(opts.lockfileDir, dirResolution.directory)
    const directory = normalizePath(path.relative(opts.deployDir, resolvedPath))
    outputResolution = { ...dirResolution, directory }
  } else if (inputResolution.type === 'git' || inputResolution.type === 'variations') {
    outputResolution = inputResolution
  } else if (inputResolution.type && typeof inputResolution.type === 'string') {
    // Custom resolution type - pass through as-is
    outputResolution = inputResolution
  } else {
    throw new Error(`Unknown resolution type: ${JSON.stringify(inputResolution)}`)
  }

  return {
    ...inputSnapshot,
    resolution: outputResolution,
    dependencies: convertResolvedDependencies(inputSnapshot.dependencies, opts),
    optionalDependencies: convertResolvedDependencies(inputSnapshot.optionalDependencies, opts),
  }
}

function convertProjectSnapshotToPackageSnapshot (projectSnapshot: ProjectSnapshot, opts: ConvertOptions): PackageSnapshot {
  const resolution: DirectoryResolution = {
    type: 'directory',
    directory: normalizePath(path.relative(opts.deployDir, opts.projectRootDirRealPath)),
  }
  const dependencies = convertResolvedDependencies(projectSnapshot.dependencies, opts)
  const optionalDependencies = convertResolvedDependencies(projectSnapshot.optionalDependencies, opts)
  return {
    dependencies,
    optionalDependencies,
    resolution,
  }
}

function convertResolvedDependencies (
  input: ResolvedDependencies | undefined,
  opts: Pick<ConvertOptions, 'allProjects' | 'deployedProjectRealPath' | 'lockfileDir' | 'projectRootDirRealPath'>
): ResolvedDependencies | undefined {
  if (!input) return undefined
  const output: ResolvedDependencies = {}

  for (const key in input) {
    const version = input[key]
    const resolveResult = resolveLinkOrFile(version, opts)
    if (!resolveResult) {
      output[key] = version
      continue
    }

    if (resolveResult.resolvedPath === opts.deployedProjectRealPath) {
      output[key] = 'link:.' // the path is relative to the lockfile dir, which means '.' would reference the deploy dir
      continue
    }

    resolveResult.packageName ??= key
    output[key] = createFileUrlDepPath(resolveResult, opts.allProjects)
  }

  return output
}

interface ResolveLinkOrFileResult {
  scheme: 'link:' | 'file:'
  resolvedPath: string
  suffix?: string
  packageName?: string
}

function resolveLinkOrFile (pkgVer: string, opts: Pick<ConvertOptions, 'lockfileDir' | 'projectRootDirRealPath'>): ResolveLinkOrFileResult | undefined {
  const { lockfileDir, projectRootDirRealPath } = opts

  function resolveScheme (scheme: ResolveLinkOrFileResult['scheme'], base: string): ResolveLinkOrFileResult | undefined {
    if (!pkgVer.startsWith(scheme)) return undefined
    const { id, peerDepGraphHash: suffix } = dp.parseDepPath(pkgVer.slice(scheme.length))
    const resolvedPath = path.resolve(base, id)
    return { scheme, resolvedPath, suffix }
  }

  const resolveSchemeResult = resolveScheme('file:', lockfileDir) ?? resolveScheme('link:', projectRootDirRealPath)
  if (resolveSchemeResult) return resolveSchemeResult

  const { name, nonSemverVersion, patchHash, peerDepGraphHash, version } = dp.parse(pkgVer)
  if (!nonSemverVersion) return undefined

  if (version) {
    throw new Error(`Something goes wrong, version should be undefined but isn't: ${version}`)
  }

  const parseResult = resolveLinkOrFile(nonSemverVersion, opts)
  if (!parseResult) return undefined

  if (parseResult.suffix) {
    throw new Error(`Something goes wrong, suffix should be undefined but isn't: ${parseResult.suffix}`)
  }

  parseResult.suffix = `${patchHash ?? ''}${peerDepGraphHash ?? ''}`
  parseResult.packageName = name

  return parseResult
}

function createFileUrlDepPath (
  { resolvedPath, suffix, packageName }: Pick<ResolveLinkOrFileResult, 'resolvedPath' | 'suffix' | 'packageName'>,
  allProjects: CreateDeployFilesOptions['allProjects']
): DepPath {
  const depFileUrl = url.pathToFileURL(resolvedPath).toString()
  const project = allProjects.find(project => project.rootDirRealPath === resolvedPath)
  const name = project?.manifest.name ?? packageName ?? path.basename(resolvedPath)
  return `${name}@${depFileUrl}${suffix ?? ''}` as DepPath
}
