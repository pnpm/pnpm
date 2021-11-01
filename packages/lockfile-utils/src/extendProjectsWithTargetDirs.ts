import path from 'path'
import { Lockfile } from '@pnpm/lockfile-types'
import { depPathToFilename } from 'dependency-path'
import fromPairs from 'ramda/src/fromPairs'

export default function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: string }>,
  lockfile: Lockfile,
  ctx: {
    lockfileDir: string
    virtualStoreDir: string
  }
) {
  const projectsById: Record<string, T & { targetDirs: string[], stages?: string[] }> =
    fromPairs(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))
  Object.entries(lockfile.packages ?? {})
    .forEach(([depPath, pkg]) => {
      if (pkg.resolution?.['type'] !== 'directory') return
      const pkgId = pkg.id ?? depPath
      const importerId = pkgId.replace(/^file:/, '')
      if (projectsById[importerId] == null) return
      const localLocation = path.join(ctx.virtualStoreDir, depPathToFilename(depPath, ctx.lockfileDir), 'node_modules', pkg.name!)
      projectsById[importerId].targetDirs.push(localLocation)
      projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
    })
  return Object.values(projectsById)
}
