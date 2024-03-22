import path from 'node:path'

import type { Lockfile } from '@pnpm/types'
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
      ? (depPath: string, pkgName: string): string[] => {
        return [
          path.join(
            ctx.virtualStoreDir,
            depPathToFilename(depPath),
            'node_modules',
            pkgName
          ),
        ];
      }
      : (depPath: string): string[] => {
        return ctx.pkgLocationsByDepPath?.[depPath] ?? [];
      }

  const projectsById: Record<
    string,
    T & { id: string; targetDirs: string[]; stages?: string[] }
  > = Object.fromEntries(
    projects.map((project) => {
      return [
        project.id,
        { ...project, targetDirs: [] },
      ];
    })
  )
  Object.entries(lockfile.packages ?? {}).forEach(([depPath, pkg]): void => {
    // @ts-ignore
    if (!('resolution' in pkg) || typeof pkg.resolution === 'undefined' || !('type' in pkg.resolution) || pkg.resolution.type !== 'directory' || typeof pkg.name === 'undefined' || pkg.name === '') {
      return
    }

    // @ts-ignore
    const pkgId = pkg.id ?? depPath

    const importerId = pkgId.replace(/^file:/, '')

    if (projectsById[importerId] == null) {
      return
    }

    // @ts-ignore
    const localLocations = getLocalLocations(depPath, pkg.name)

    if (!localLocations) {
      return
    }

    const project = projectsById[importerId]

    if (typeof project !== 'undefined') {
      project.targetDirs.push(...localLocations)

      project.stages = [
        'preinstall',
        'install',
        'postinstall',
        'prepare',
        'prepublishOnly',
      ]
    }
  })

  return Object.values(projectsById) as Array<
    T & { id: string; stages: string[]; targetDirs: string[] }
  >
}
