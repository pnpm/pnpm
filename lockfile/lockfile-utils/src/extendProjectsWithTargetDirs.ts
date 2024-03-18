import path from 'node:path'
import type { Lockfile } from '@pnpm/lockfile-types'
import { depPathToFilename } from '@pnpm/dependency-path'

type GetLocalLocations = (depPath: string, pkgName: string) => string[]

export function extendProjectsWithTargetDirs<T>(
  projects: Array<T & { id: string }>,
  lockfile: Lockfile,
  ctx: {
    virtualStoreDir: string
    pkgLocationsByDepPath?: Record<string, string[]>
  }
): Array<T & { id: string; stages: string[]; targetDirs: string[] }> {
  const getLocalLocations: GetLocalLocations | undefined =
    typeof ctx.pkgLocationsByDepPath === 'undefined'
      ? (depPath: string, pkgName: string): string[] => [
        path.join(
          ctx.virtualStoreDir,
          depPathToFilename(depPath),
          'node_modules',
          pkgName
        ),
      ]
      : (depPath: string): string[] => {
        return ctx.pkgLocationsByDepPath?.[depPath] ?? [];
      }
  const projectsById: Record<
    string,
    T & { id: string; targetDirs: string[]; stages?: string[] }
  > = Object.fromEntries(
    projects.map((project) => [
      project.id,
      { ...project, targetDirs: [] as string[] },
    ])
  )
  Object.entries(lockfile.packages ?? {}).forEach(([depPath, pkg]) => {
    if (!('type' in pkg.resolution) || pkg.resolution.type !== 'directory' || typeof pkg.name === 'undefined' || pkg.name === '') {
      return
    }
    const pkgId = pkg.id ?? depPath
    const importerId = pkgId.replace(/^file:/, '')
    if (projectsById[importerId] == null) {
      return
    }
    const localLocations = getLocalLocations(depPath, pkg.name)
    if (!localLocations) {
      return
    }
    projectsById[importerId].targetDirs.push(...localLocations)
    projectsById[importerId].stages = [
      'preinstall',
      'install',
      'postinstall',
      'prepare',
      'prepublishOnly',
    ]
  })
  return Object.values(projectsById) as Array<
    T & { id: string; stages: string[]; targetDirs: string[] }
  >
}
