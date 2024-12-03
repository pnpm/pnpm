import path from 'path'
import url from 'url'
import normalizePath from 'normalize-path'
import { globalWarn } from '@pnpm/logger'
import {
  type DirectoryResolution,
  type Lockfile,
  type PackageSnapshot,
  type PackageSnapshots,
  type ProjectSnapshot,
} from '@pnpm/lockfile.types'
import {
  type DependenciesField,
  type DepPath,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'

const DEPENDENCIES_FIELD = ['dependencies', 'devDependencies', 'optionalDependencies'] as const satisfies DependenciesField[]
const REPLACEABLE_PREFIXES = ['link:', 'file:'] as const

export type DeployManifest = Pick<ProjectManifest, 'name' | 'version' | DependenciesField>

export interface CreateDeployFilesOptions {
  lockfile: Lockfile
  lockfileDir: string
  manifest: DeployManifest
  projectId: ProjectId
  targetDir: string // necessary to make `packageSnapshot.resolution.directory` a relative path
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
  targetDir,
}: CreateDeployFilesOptions): DeployFiles {
  const inputSnapshot = lockfile.importers[projectId]

  const resolvedProjectSnapshots: Record<string, ProjectSnapshot | undefined> = {}
  for (const [id, snapshot] of Object.entries(lockfile.importers)) {
    const resolvedPath = path.resolve(lockfileDir, id)
    resolvedProjectSnapshots[resolvedPath] = snapshot
  }

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

  for (const field of DEPENDENCIES_FIELD) {
    const targetDependencies = targetSnapshot[field] ?? {}
    const targetSpecifiers = targetSnapshot.specifiers
    const inputDependencies = inputSnapshot[field] ?? {}
    for (const name in inputDependencies) {
      const spec = inputDependencies[name]
      const prefix = REPLACEABLE_PREFIXES.find(prefix => spec.startsWith(prefix))
      if (!prefix) continue
      const targetPath = spec.slice(prefix.length)
      const targetRealPath = path.resolve(lockfileDir, projectId, targetPath) // importer IDs are relative to its project dir
      const targetFileUrl = url.pathToFileURL(targetRealPath).toString()
      targetSpecifiers[name] = targetDependencies[name] = targetFileUrl

      const packageSnapshot = getPackageSnapshot({
        lockfile,
        name,
        resolvedProjectSnapshots,
        spec,
        targetDir,
        targetRealPath,
      })
      if (packageSnapshot) {
        const targetDepPath = `${name}@${targetFileUrl}` as DepPath
        targetPackageSnapshots[targetDepPath] = packageSnapshot
      } else {
        globalWarn(`Entry ${name}@${spec} has neither a project snapshot nor a package snapshot in the lockfile`)
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
      name: manifest.name,
      version: manifest.version,
      dependencies: targetSnapshot.dependencies,
      devDependencies: targetSnapshot.devDependencies,
      optionalDependencies: targetSnapshot.optionalDependencies,
    },
  }
}

interface GetPackageSnapshotOptions {
  lockfile: Pick<Lockfile, 'packages'>
  name: string
  resolvedProjectSnapshots: Record<string, ProjectSnapshot | undefined>
  spec: string
  targetDir: string // necessary to make `packageSnapshot.resolution.directory` a relative path
  targetRealPath: string
}

function getPackageSnapshot ({
  lockfile,
  name,
  resolvedProjectSnapshots,
  spec,
  targetDir,
  targetRealPath,
}: GetPackageSnapshotOptions): PackageSnapshot | undefined {
  const projectSnapshot = resolvedProjectSnapshots[targetRealPath]
  if (projectSnapshot) {
    const directory = normalizePath(
      // path.relative is necessary because `pnpm install` doesn't join absolute path correctly
      path.relative(targetDir, targetRealPath)
    )
    const resolution: DirectoryResolution = {
      type: 'directory',
      directory,
    }
    return { ...projectSnapshot, resolution }
  }

  const depPath = `${name}@${spec}` as DepPath
  return lockfile.packages?.[depPath]
}
