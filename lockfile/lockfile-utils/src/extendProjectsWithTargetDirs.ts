import path from 'path'
import { type Lockfile, type TarballResolution } from '@pnpm/lockfile.types'
import { depPathToFilename } from '@pnpm/dependency-path'
import { type ProjectId, type DepPath } from '@pnpm/types'
import { packageIdFromSnapshot } from './packageIdFromSnapshot'
import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot'

type GetLocalLocations = (depPath: DepPath, pkgName: string) => string[]

export function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: ProjectId }>,
  lockfile: Lockfile,
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
  Object.entries(lockfile.packages ?? {})
    .forEach(([depPath, pkg]) => {
      if ((pkg.resolution as TarballResolution)?.type !== 'directory') return
      const pkgId = packageIdFromSnapshot(depPath as DepPath, pkg)
      const { name: pkgName } = nameVerFromPkgSnapshot(depPath, pkg)
      const importerId = pkgId.replace(/^file:/, '') as ProjectId
      if (projectsById[importerId] == null) return
      const localLocations = getLocalLocations(depPath as DepPath, pkgName)
      if (!localLocations) return
      projectsById[importerId].targetDirs.push(...localLocations)
      projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
    })
  return Object.values(projectsById) as Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }>
}
