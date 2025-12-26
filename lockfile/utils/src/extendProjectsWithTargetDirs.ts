import path from 'path'
import { type LockfileObject, type TarballResolution } from '@pnpm/lockfile.types'
import { depPathToFilename, parse as parseDepPath } from '@pnpm/dependency-path'
import { type ProjectId, type DepPath } from '@pnpm/types'
import { packageIdFromSnapshot } from './packageIdFromSnapshot.js'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

export function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: ProjectId }>,
  lockfile: LockfileObject,
  ctx: {
    virtualStoreDir: string
    directoryDepsByDepPath?: Map<string, string[]>
    virtualStoreDirMaxLength: number
  }
): Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }> {
  const projectsById: Record<ProjectId, T & { id: ProjectId, targetDirs: string[], stages?: string[] }> =
    Object.fromEntries(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))

  if (ctx.directoryDepsByDepPath) {
    // When directoryDepsByDepPath is provided (from dependency graph), use it directly
    // It already contains only directory deps with their resolved locations
    for (const [depPath, locations] of ctx.directoryDepsByDepPath) {
      const parsed = parseDepPath(depPath)
      if (!parsed.name || !parsed.nonSemverVersion?.startsWith('file:')) continue
      const importerId = parsed.nonSemverVersion.replace(/^file:/, '') as ProjectId
      if (projectsById[importerId] == null) continue
      // Dedupe: only add locations that aren't already tracked
      for (const location of locations) {
        if (!projectsById[importerId].targetDirs.includes(location)) {
          projectsById[importerId].targetDirs.push(location)
        }
      }
      projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
    }
  } else {
    // Fallback: iterate lockfile.packages and compute locations
    for (const [depPath, pkg] of Object.entries(lockfile.packages ?? {})) {
      if ((pkg.resolution as TarballResolution)?.type !== 'directory') continue
      const pkgId = packageIdFromSnapshot(depPath as DepPath, pkg)
      const { name: pkgName } = nameVerFromPkgSnapshot(depPath, pkg)
      const importerId = pkgId.replace(/^file:/, '') as ProjectId
      if (projectsById[importerId] == null) continue
      const dir = path.join(ctx.virtualStoreDir, depPathToFilename(depPath as DepPath, ctx.virtualStoreDirMaxLength), 'node_modules', pkgName)
      projectsById[importerId].targetDirs.push(dir)
      projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
    }
  }

  return Object.values(projectsById) as Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }>
}
