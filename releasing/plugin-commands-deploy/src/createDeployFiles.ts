import path from 'path'
import url from 'url'
import normalizePath from 'normalize-path'
import pick from 'ramda/src/pick'
import {
  type DirectoryResolution,
  type LockfileObject,
  type PackageSnapshot,
  type PackageSnapshots,
  type ProjectSnapshot,
  type ResolvedDependencies,
} from '@pnpm/lockfile.types'
import {
  type DependenciesField,
  type DepPath,
  type Project,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'

const DEPENDENCIES_FIELD = ['dependencies', 'devDependencies', 'optionalDependencies'] as const satisfies DependenciesField[]

const INHERITED_MANIFEST_KEYS = [
  'name',
  'description',
  'version',
  'private',
  'author',
  'bin',
  'scripts',
  'packageManager',
  'dependenciesMeta',
  'peerDependenciesMeta',
] as const satisfies Array<keyof ProjectManifest>

export type DeployManifest = Pick<ProjectManifest, typeof INHERITED_MANIFEST_KEYS[number] | DependenciesField | 'pnpm'>

export interface CreateDeployFilesOptions {
  allProjects: Array<Pick<Project, 'manifest' | 'rootDirRealPath'>>
  deployDir: string
  lockfile: LockfileObject
  lockfileDir: string
  manifest: DeployManifest
  projectId: ProjectId
  rootProjectManifestDir: string
}

export interface DeployFiles {
  lockfile: LockfileObject
  manifest: DeployManifest
}

export function createDeployFiles ({
  allProjects,
  deployDir,
  lockfile,
  lockfileDir,
  manifest,
  projectId,
  rootProjectManifestDir,
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

  const targetPackageSnapshots: PackageSnapshots = {}
  for (const name in lockfile.packages) {
    const depPath = name as DepPath
    const inputSnapshot = lockfile.packages[depPath]
    targetPackageSnapshots[depPath] = convertPackageSnapshot(inputSnapshot, {
      allProjects,
      deployedProjectRealPath,
      lockfileDir,
      projectRootDirRealPath: rootProjectManifestDir,
    })
  }

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
    const depPath = createFileUrlDepPath(projectRootDirRealPath, allProjects)
    targetPackageSnapshots[depPath] = packageSnapshot
  }

  for (const field of DEPENDENCIES_FIELD) {
    const targetDependencies = targetSnapshot[field] ?? {}
    const targetSpecifiers = targetSnapshot.specifiers
    const inputDependencies = inputSnapshot[field] ?? {}
    for (const name in inputDependencies) {
      const spec = inputDependencies[name]
      const targetRealPath = resolveLinkOrFile(spec, {
        lockfileDir,
        projectRootDirRealPath: path.resolve(lockfileDir, projectId),
      })

      if (!targetRealPath) {
        targetSpecifiers[name] = targetDependencies[name] = spec
        continue
      }

      targetSpecifiers[name] = targetDependencies[name] =
        targetRealPath === deployedProjectRealPath ? 'link:.' : createFileUrlDepPath(targetRealPath, allProjects)
    }
  }

  const result: DeployFiles = {
    lockfile: {
      ...lockfile,
      overrides: undefined, // the effects of package overrides should already be part of the package snapshots
      patchedDependencies: undefined,
      packageExtensionsChecksum: undefined, // the effects of the package extensions should already be part of the package snapshots
      pnpmfileChecksum: undefined, // the effects of the pnpmfile should already be part of the package snapshots
      importers: {
        ['.' as ProjectId]: targetSnapshot,
      },
      packages: targetPackageSnapshots,
    },
    manifest: {
      ...pick(INHERITED_MANIFEST_KEYS, manifest),
      dependencies: targetSnapshot.dependencies,
      devDependencies: targetSnapshot.devDependencies,
      optionalDependencies: targetSnapshot.optionalDependencies,
      pnpm: {
        ...manifest.pnpm,
        overrides: undefined, // the effects of package overrides should already be part of the package snapshots
        patchedDependencies: undefined,
        packageExtensions: undefined, // the effects of the package extensions should already be part of the package snapshots
      },
    },
  }

  if (lockfile.patchedDependencies) {
    result.lockfile.patchedDependencies = {}
    result.manifest.pnpm!.patchedDependencies = {}

    for (const name in lockfile.patchedDependencies) {
      const patchInfo = lockfile.patchedDependencies[name]
      const resolvedPath = path.resolve(rootProjectManifestDir, patchInfo.path)
      const relativePath = normalizePath(path.relative(deployDir, resolvedPath))
      result.manifest.pnpm!.patchedDependencies[name] = relativePath
      result.lockfile.patchedDependencies[name] = {
        hash: patchInfo.hash,
        path: relativePath,
      }
    }
  }

  return result
}

interface ConvertOptions {
  allProjects: CreateDeployFilesOptions['allProjects']
  deployedProjectRealPath: string
  projectRootDirRealPath: string
  lockfileDir: string
}

function convertPackageSnapshot (inputSnapshot: PackageSnapshot, opts: ConvertOptions): PackageSnapshot {
  const dependencies = convertResolvedDependencies(inputSnapshot.dependencies, opts)
  const optionalDependencies = convertResolvedDependencies(inputSnapshot.optionalDependencies, opts)
  return {
    ...inputSnapshot,
    dependencies,
    optionalDependencies,
  }
}

function convertProjectSnapshotToPackageSnapshot (
  projectSnapshot: ProjectSnapshot,
  opts: ConvertOptions & { deployDir: string }
): PackageSnapshot {
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

function convertResolvedDependencies (input: ResolvedDependencies | undefined, opts: ConvertOptions): ResolvedDependencies | undefined {
  if (!input) return undefined
  const output: ResolvedDependencies = {}

  for (const key in input) {
    const spec = input[key]
    const depRealPath = resolveLinkOrFile(spec, opts)
    if (!depRealPath) {
      output[key] = spec
      continue
    }

    if (depRealPath === opts.deployedProjectRealPath) {
      output[key] = 'link:.' // the path is relative to the lockfile dir, which means '.' would reference the deploy dir
      continue
    }

    output[key] = createFileUrlDepPath(depRealPath, opts.allProjects)
  }

  return output
}

function resolveLinkOrFile (spec: string, opts: Pick<ConvertOptions, 'lockfileDir' | 'projectRootDirRealPath'>): string | undefined {
  // try parsing `spec` as either @scope/name@pref or name@pref
  const renamed = /^@(?<scope>[^@]+)\/(?<name>[^@]+)@(?<pref>.+)$/.exec(spec) ?? /^(?<name>[^@]+)@(?<pref>.+)$/.exec(spec)
  if (renamed) return resolveLinkOrFile(renamed.groups!.pref, opts)

  const { lockfileDir, projectRootDirRealPath } = opts

  if (spec.startsWith('link:')) {
    const targetPath = spec.slice('link:'.length)
    return path.resolve(projectRootDirRealPath, targetPath)
  }

  if (spec.startsWith('file:')) {
    const targetPath = spec.slice('file:'.length)
    return path.resolve(lockfileDir, targetPath)
  }

  return undefined
}

function createFileUrlDepPath (
  depRealPath: string,
  allProjects: CreateDeployFilesOptions['allProjects']
): DepPath {
  const depFileUrl = url.pathToFileURL(depRealPath).toString()
  const project = allProjects.find(project => project.rootDirRealPath === depRealPath)
  const name = project?.manifest.name ?? path.basename(depRealPath)
  return `${name}@${depFileUrl}` as DepPath
}
