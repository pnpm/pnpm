import { rootLogger } from '@pnpm/core-loggers'
import { symlinkDependency, symlinkDirectRootDependency } from '@pnpm/symlink-dependency'

export interface LinkedDirectDep {
  alias: string
  name: string
  version: string
  dir: string
  id: string
  dependencyType: 'prod' | 'dev' | 'optional'
  isExternalLink: boolean
  latest?: string
}

export interface ProjectToLink {
  dir: string
  modulesDir: string
  dependencies: LinkedDirectDep[]
}

export async function linkDirectDeps (
  projects: ProjectToLink[]
) {
  await Promise.all(projects.map(async (project) => {
    await Promise.all(project.dependencies.map(async (dep) => {
      if (dep.isExternalLink) {
        await symlinkDirectRootDependency(dep.dir, project.modulesDir, dep.alias, {
          fromDependenciesField: dep.dependencyType === 'dev' && 'devDependencies' ||
            dep.dependencyType === 'optional' && 'optionalDependencies' ||
            'dependencies',
          linkedPackage: {
            name: dep.name,
            version: dep.version,
          },
          prefix: project.dir,
        })
        return
      }
      if ((await symlinkDependency(dep.dir, project.modulesDir, dep.alias)).reused) {
        return
      }
      rootLogger.debug({
        added: {
          dependencyType: dep.dependencyType,
          id: dep.id,
          latest: dep.latest,
          name: dep.alias,
          realName: dep.name,
          version: dep.version,
        },
        prefix: project.dir,
      })
    }))
  }))
}
