import path from 'path'
import { type Lockfile, type TarballResolution } from '@pnpm/lockfile-types'
import { depPathToFilename, parse as parseDepPath } from '@pnpm/dependency-path'

type GetLocalLocations = (depPath: string, pkgName: string) => string[]

export function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: string }>,
  lockfile: Lockfile,
  ctx: {
    virtualStoreDir: string
    pkgLocationsByDepPath?: Record<string, string[]>
    virtualStoreDirMaxLength: number
  }
): Array<T & { id: string, stages: string[], targetDirs: string[] }> {
  const getLocalLocations: GetLocalLocations = ctx.pkgLocationsByDepPath != null
    ? (depPath: string) => ctx.pkgLocationsByDepPath![depPath]
    : (depPath: string, pkgName: string) => [path.join(ctx.virtualStoreDir, depPathToFilename(depPath, ctx.virtualStoreDirMaxLength), 'node_modules', pkgName)]
  const projectsById: Record<string, T & { id: string, targetDirs: string[], stages?: string[] }> =
    Object.fromEntries(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))
  Object.entries(lockfile.packages ?? {})
    .forEach(([depPath, pkg]) => {
      if ((pkg.resolution as TarballResolution)?.type !== 'directory') return
      const { name: pkgName, nonSemverVersion } = parseDepPath(depPath)
      const importerId = nonSemverVersion?.replace(/^file:/, '')
      if (!importerId || !pkgName || projectsById[importerId] == null) return
      const localLocations = getLocalLocations(depPath, pkgName)
      if (!localLocations) return
      projectsById[importerId].targetDirs.push(...localLocations)
      projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
    })
  return Object.values(projectsById) as Array<T & { id: string, stages: string[], targetDirs: string[] }>
}
