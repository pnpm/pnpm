import path from 'path'
import url from 'url'
import pick from 'ramda/src/pick'
import {
  type DirectoryResolution,
  type Lockfile,
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
  lockfile: Lockfile
  lockfileDir: string
  manifest: DeployManifest
  projectId: ProjectId
}

export interface DeployFiles {
  lockfile: Lockfile
  manifest: DeployManifest
}

export function createDeployFiles ({
  allProjects,
  lockfile,
  lockfileDir,
  manifest,
  projectId,
}: CreateDeployFilesOptions): DeployFiles {
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
    const importerRealPath = path.resolve(lockfileDir, importerPath) as ProjectRootDirRealPath
    const deployedProjectRealPath = path.resolve(lockfileDir, projectId) as ProjectRootDirRealPath
    const packageSnapshot = convertProjectSnapshotToPackageSnapshot(projectSnapshot, importerRealPath, allProjects, deployedProjectRealPath)
    const depPath = createFileUrlDepPath(importerRealPath, allProjects)
    targetPackageSnapshots[depPath] = packageSnapshot
  }

  for (const field of DEPENDENCIES_FIELD) {
    const targetDependencies = targetSnapshot[field] ?? {}
    const targetSpecifiers = targetSnapshot.specifiers
    const inputDependencies = inputSnapshot[field] ?? {}
    for (const name in inputDependencies) {
      const spec = inputDependencies[name]
      const splitPrefixResult = splitPrefix(spec)
      if (!splitPrefixResult) {
        targetSpecifiers[name] = targetDependencies[name] = spec
        continue
      }

      const { targetPath } = splitPrefixResult
      const targetRealPath = path.resolve(lockfileDir, projectId, targetPath) as ProjectRootDirRealPath // importer IDs are relative to its project dir
      targetSpecifiers[name] = targetDependencies[name] = createFileUrlDepPath(targetRealPath, allProjects)
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

function convertProjectSnapshotToPackageSnapshot (
  projectSnapshot: ProjectSnapshot,
  importerRealPath: string,
  allProjects: CreateDeployFilesOptions['allProjects'],
  deployedProjectRealPath: ProjectRootDirRealPath
): PackageSnapshot {
  const resolution: DirectoryResolution = {
    type: 'directory',
    directory: '.',
  }
  const dependencies = convertResolvedDependencies(projectSnapshot.dependencies, importerRealPath, allProjects, deployedProjectRealPath)
  const optionalDependencies = convertResolvedDependencies(projectSnapshot.optionalDependencies, importerRealPath, allProjects, deployedProjectRealPath)
  return {
    dependencies,
    optionalDependencies,
    resolution,
  }
}

function convertResolvedDependencies (
  input: ResolvedDependencies | undefined,
  importerRealPath: string,
  allProjects: CreateDeployFilesOptions['allProjects'],
  deployedProjectRealPath: ProjectRootDirRealPath
): ResolvedDependencies | undefined {
  if (!input) return undefined
  const output: ResolvedDependencies = {}

  for (const key in input) {
    const spec = input[key]
    const splitPrefixResult = splitPrefix(spec)
    if (!splitPrefixResult) {
      output[key] = spec
      continue
    }

    const { targetPath } = splitPrefixResult
    const depRealPath = path.resolve(importerRealPath, targetPath) as ProjectRootDirRealPath
    if (['', '.'].includes(path.relative(importerRealPath, depRealPath))) {
      output[key] = 'link:.'
      continue
    }

    if (depRealPath === deployedProjectRealPath) {
      output[key] = '../../..' // which is the deployed project location relative to the dependency dir within the virtual dir
      continue
    }

    output[key] = createFileUrlDepPath(depRealPath, allProjects)
  }

  return output
}

interface SplitPrefixResult {
  prefix: typeof REPLACEABLE_PREFIXES[number]
  targetPath: string
}

function splitPrefix (spec: string): SplitPrefixResult | undefined {
  const prefix = REPLACEABLE_PREFIXES.find(prefix => spec.startsWith(prefix))
  if (!prefix) return undefined
  const targetPath = spec.slice(prefix.length)
  return { prefix, targetPath }
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
