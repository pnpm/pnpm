import path from 'path'
import { type LockfileObject, type TarballResolution } from '@pnpm/lockfile.types'
import { depPathToFilename, parse as parseDepPath } from '@pnpm/dependency-path'
import { type ProjectId, type DepPath } from '@pnpm/types'
import { packageIdFromSnapshot } from './packageIdFromSnapshot.js'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

type GetLocalLocations = (depPath: DepPath, pkgName: string) => string[]

export function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: ProjectId }>,
  lockfile: LockfileObject,
  ctx: {
    virtualStoreDir: string
    pkgLocationsByDepPath?: Record<DepPath, string[]>
    virtualStoreDirMaxLength: number
  }
): Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }> {
  const getLocalLocations: GetLocalLocations = ctx.pkgLocationsByDepPath != null
    ? (depPath: DepPath) => ctx.pkgLocationsByDepPath![depPath]
    : (depPath: DepPath, pkgName: string) => [path.join(ctx.virtualStoreDir, depPathToFilename(depPath, ctx.virtualStoreDirMaxLength), 'node_modules', pkgName)]
  const projectsById: Record<ProjectId, T & { id: ProjectId, targetDirs: string[], stages?: string[] }> =
    Object.fromEntries(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))

  // First, check lockfile.packages for directory dependencies (original behavior)
  for (const [depPath, pkg] of Object.entries(lockfile.packages ?? {})) {
    if ((pkg.resolution as TarballResolution)?.type !== 'directory') continue
    const pkgId = packageIdFromSnapshot(depPath as DepPath, pkg)
    const { name: pkgName } = nameVerFromPkgSnapshot(depPath, pkg)
    const importerId = pkgId.replace(/^file:/, '') as ProjectId
    if (projectsById[importerId] == null) continue
    const localLocations = getLocalLocations(depPath as DepPath, pkgName)
    if (!localLocations) continue
    projectsById[importerId].targetDirs.push(...localLocations)
    projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
  }

  // If pkgLocationsByDepPath is provided, also check it for file: deps that may not be in lockfile.packages
  // This handles cases where the dependency graph has the info but lockfile.packages doesn't
  if (ctx.pkgLocationsByDepPath) {
    for (const depPath of Object.keys(ctx.pkgLocationsByDepPath) as DepPath[]) {
      if (!depPath.includes('file:')) continue
      // Extract project ID from depPath like "project-1@file:project-1"
      const parsed = parseDepPath(depPath)
      if (!parsed.name || !parsed.nonSemverVersion?.startsWith('file:')) continue
      const importerId = parsed.nonSemverVersion.replace(/^file:/, '') as ProjectId
      if (projectsById[importerId] == null) continue
      // Skip if already processed from lockfile.packages
      if (projectsById[importerId].targetDirs.length > 0) continue
      const localLocations = ctx.pkgLocationsByDepPath[depPath]
      if (!localLocations) continue
      projectsById[importerId].targetDirs.push(...localLocations)
      projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
    }
  }

  return Object.values(projectsById) as Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }>
}
