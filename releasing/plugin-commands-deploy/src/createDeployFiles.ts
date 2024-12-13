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
  type ProjectId,
  type ProjectManifest,
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
    const importerRealPath = path.resolve(lockfileDir, importerPath)
    const packageSnapshot = convertProjectSnapshotToPackageSnapshot(projectSnapshot, importerRealPath)
    const name = path.basename(importerPath) // TODO: use real name from manifest from opts.allProjects
    const targetFileUrl = url.pathToFileURL(importerRealPath).toString()
    const targetDepPath = `${name}@${targetFileUrl}` as DepPath
    targetPackageSnapshots[targetDepPath] = packageSnapshot
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
      const targetRealPath = path.resolve(lockfileDir, projectId, targetPath) // importer IDs are relative to its project dir
      const targetFileUrl = url.pathToFileURL(targetRealPath).toString()
      targetSpecifiers[name] = targetDependencies[name] = targetFileUrl
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

function convertProjectSnapshotToPackageSnapshot (projectSnapshot: ProjectSnapshot, importerRealPath: string): PackageSnapshot {
  const resolution: DirectoryResolution = {
    type: 'directory',
    directory: '.',
  }
  const dependencies = convertResolvedDependencies(projectSnapshot.dependencies, importerRealPath)
  const optionalDependencies = convertResolvedDependencies(projectSnapshot.optionalDependencies, importerRealPath)
  return {
    dependencies,
    optionalDependencies,
    resolution,
  }
}

function convertResolvedDependencies (input: ResolvedDependencies | undefined, importerRealPath: string): ResolvedDependencies | undefined {
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
    const depRealPath = path.resolve(importerRealPath, targetPath)
    if (['', '.'].includes(path.relative(importerRealPath, depRealPath))) {
      output[key] = 'link:.'
      continue
    }

    output[key] = url.pathToFileURL(depRealPath).toString()
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
