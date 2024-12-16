import path from 'path'
import url from 'url'
import pick from 'ramda/src/pick'
import normalizePath from 'normalize-path'
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
  allProjects: Array<Pick<Project, 'manifest' | 'rootDir' | 'rootDirRealPath'>>
  lockfile: LockfileObject
  lockfileDir: string
  selectedProject: Pick<Project, 'manifest' | 'rootDir'>
}

export interface DeployFiles {
  lockfile: LockfileObject
  manifest: DeployManifest
}

export function createDeployFiles ({
  allProjects,
  lockfile,
  lockfileDir,
  selectedProject,
}: CreateDeployFilesOptions): DeployFiles {
  const selectedProjectId = normalizePath(path.relative(lockfileDir, selectedProject.rootDir)) as ProjectId
  const inputSnapshot = lockfile.importers[selectedProjectId]

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

  for (const project of allProjects) {
    if (project.rootDir === selectedProject.rootDir) continue
    const projectId = (normalizePath(path.relative(lockfileDir, project.rootDir)) || '.') as ProjectId
    const projectSnapshot = lockfile.importers[projectId]
    const packageSnapshot = convertProjectSnapshotToPackageSnapshot(projectSnapshot, project, selectedProject)
    const depPath = createFileUrlDepPath(project)
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
      const depRealPath = path.resolve(selectedProject.rootDir, targetPath) as ProjectRootDirRealPath
      const depProject = allProjects.find(project => project.rootDirRealPath === depRealPath)
      if (depProject) {
        targetSpecifiers[name] = targetDependencies[name] = createFileUrlDepPath(depProject)
      } else {
        // if this branch is reached, it means that there is a bug in pnpm that needs to be fixed
        throw new Error(`Cannot find any project in opts.allProject whose rootDirRealPath is '${depRealPath}'`)
      }
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
      ...pick(INHERITED_MANIFEST_KEYS, selectedProject.manifest),
      dependencies: targetSnapshot.dependencies,
      devDependencies: targetSnapshot.devDependencies,
      optionalDependencies: targetSnapshot.optionalDependencies,
    },
  }
}

function convertProjectSnapshotToPackageSnapshot (
  projectSnapshot: ProjectSnapshot,
  inputProject: Pick<Project, 'manifest' | 'rootDir'>,
  selectedProject: Pick<Project, 'manifest' | 'rootDir'>
): PackageSnapshot {
  const resolution: DirectoryResolution = {
    type: 'directory',
    directory: '.',
  }
  const dependencies = convertResolvedDependencies(projectSnapshot.dependencies, inputProject, selectedProject)
  const optionalDependencies = convertResolvedDependencies(projectSnapshot.optionalDependencies, inputProject, selectedProject)
  return {
    dependencies,
    optionalDependencies,
    resolution,
  }
}

function convertResolvedDependencies (
  input: ResolvedDependencies | undefined,
  inputProject: Pick<Project, 'manifest' | 'rootDir'>,
  selectedProject: Pick<Project, 'manifest' | 'rootDir'>
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
    const depRealPath = path.resolve(inputProject.rootDir, targetPath)
    if (['', '.'].includes(path.relative(inputProject.rootDir, depRealPath))) {
      output[key] = 'link:.'
      continue
    }

    if (depRealPath === selectedProject.rootDir) {
      output[key] = '../../..' // which is the deployed project location relative to the dependency dir within the virtual dir
      continue
    }

    output[key] = createFileUrlDepPath(inputProject)
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

function createFileUrlDepPath (inputProject: Pick<Project, 'manifest' | 'rootDir'>): DepPath {
  const depFileUrl = url.pathToFileURL(inputProject.rootDir).toString()
  const name = inputProject.manifest.name ?? path.basename(inputProject.rootDir)
  return `${name}@${depFileUrl}` as DepPath
}
