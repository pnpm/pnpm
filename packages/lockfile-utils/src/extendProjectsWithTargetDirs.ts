import path from 'path'
import { Lockfile } from '@pnpm/lockfile-types'
import { depPathToFilename } from 'dependency-path'
import fromPairs from 'ramda/src/fromPairs'

export default function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: string }>,
  lockfile: Lockfile,
  ctx: {
    virtualStoreDir: string
    pkgLocationByDepPath?: Record<string, string>
  }
): Array<T & { id: string, stages: string[], targetDirs: string[] }> {
  const getLocalLocation = ctx.pkgLocationByDepPath != null
    ? (depPath: string) => ctx.pkgLocationByDepPath![depPath]
    : (depPath: string, pkgName: string) => path.join(ctx.virtualStoreDir, depPathToFilename(depPath), 'node_modules', pkgName)
  const projectsById: Record<string, T & { id: string, targetDirs: string[], stages?: string[] }> =
    fromPairs(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))
  Object.entries(lockfile.packages ?? {})
    .forEach(([depPath, pkg]) => {
      if (pkg.resolution?.['type'] !== 'directory') return
      const pkgId = pkg.id ?? depPath
      const importerId = pkgId.replace(/^file:/, '')
      if (projectsById[importerId] == null) return
      const localLocation = getLocalLocation(depPath, pkg.name!)
      projectsById[importerId].targetDirs.push(localLocation)
      projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
    })
  return Object.values(projectsById) as Array<T & { id: string, stages: string[], targetDirs: string[] }>
}
