import path from 'path'
import url from 'url'
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
  type ProjectRootDirRealPath,
} from '@pnpm/types'

const DEPENDENCIES_FIELD = ['dependencies', 'devDependencies', 'optionalDependencies'] as const satisfies DependenciesField[]
const REPLACEABLE_PREFIXES = ['link:', 'file:'] as const

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

export type DeployManifest = Pick<ProjectManifest, typeof INHERITED_MANIFEST_KEYS[number] | DependenciesField>

export interface CreateDeployFilesOptions {
  allProjects: Array<Pick<Project, 'manifest' | 'rootDirRealPath'>>
  lockfile: LockfileObject
  lockfileDir: string
  manifest: DeployManifest
  projectId: ProjectId
}

export interface DeployFiles {
  lockfile: LockfileObject
  manifest: DeployManifest
}

export function createDeployFiles ({
  allProjects,
  lockfile,
  lockfileDir,
  manifest,
  projectId,
}: CreateDeployFilesOptions): DeployFiles {
  const deployedProjectRealPath = path.resolve(lockfileDir, projectId) as ProjectRootDirRealPath
  const inputSnapshot = lockfile.importers[projectId]

  const targetSnapshot: ProjectSnapshot = {
    ...inputSnapshot,
    specifiers: {},
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
  }

  const targetPackageSnapshots: PackageSnapshots = {
    ...lockfile.packages,
  }

  for (const importerPath in lockfile.importers) {
    if (importerPath === projectId) continue
    const projectSnapshot = lockfile.importers[importerPath as ProjectId]
    const projectRootDirRealPath = path.resolve(lockfileDir, importerPath) as ProjectRootDirRealPath
    const packageSnapshot = convertProjectSnapshotToPackageSnapshot(projectSnapshot, {
      allProjects,
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
        targetRealPath === deployedProjectRealPath ? 'link:.' : createFileUrlDepPath(targetRealPath as ProjectRootDirRealPath, allProjects)
    }
  }

  return {
    lockfile: {
      ...lockfile,
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
    },
  }
}

interface ConvertOptions {
  allProjects: CreateDeployFilesOptions['allProjects']
  deployedProjectRealPath: ProjectRootDirRealPath
  projectRootDirRealPath: string
  lockfileDir: string
}

function convertProjectSnapshotToPackageSnapshot (projectSnapshot: ProjectSnapshot, opts: ConvertOptions): PackageSnapshot {
  const resolution: DirectoryResolution = {
    type: 'directory',
    directory: '.',
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

    output[key] = createFileUrlDepPath(depRealPath as ProjectRootDirRealPath, opts.allProjects)
  }

  return output
}

function resolveLinkOrFile (spec: string, opts: Pick<ConvertOptions, 'lockfileDir' | 'projectRootDirRealPath'>): string | undefined {
  // try parsing `spec` as either @scope/name@pref or name@pref
  const renamed = /^@(?<scope>[^@]+)\/(?<name>[^@]+)@(?<pref>.+)$/.exec(spec) ?? /^(?<name>[^@])+@(?<pref>.+)$/.exec(spec)
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
  depRealPath: ProjectRootDirRealPath,
  allProjects: CreateDeployFilesOptions['allProjects']
): DepPath {
  const depFileUrl = url.pathToFileURL(depRealPath).toString()
  const project = allProjects.find(project => project.rootDirRealPath === depRealPath)
  const name = project?.manifest.name ?? path.basename(depRealPath)
  return `${name}@${depFileUrl}` as DepPath
}
