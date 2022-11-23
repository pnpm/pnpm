import { rootLogger } from '@pnpm/core-loggers'
import { symlinkDependency, symlinkDirectRootDependency } from '@pnpm/symlink-dependency'

export interface LinkedDirectDep {
  alias: string
  name: string
  version: string
  depLocation: string
  id: string
  dependencyType: 'prod' | 'dev' | 'optional'
  isLinked: boolean
}

export interface ProjectToLink {
  projectId: string
  projectDir: string
  modulesDir: string
  dependencies: LinkedDirectDep[]
}

export async function linkDirectDeps (
  projects: ProjectToLink[]
) {
  await Promise.all(projects.map(async (project) => {
    await Promise.all(project.dependencies.map(async (dep) => {
      if (dep.isLinked) {
        await symlinkDirectRootDependency(dep.depLocation, project.modulesDir, dep.alias, {
          fromDependenciesField: dep.dependencyType === 'dev' && 'devDependencies' ||
            dep.dependencyType === 'optional' && 'optionalDependencies' ||
            'dependencies',
          linkedPackage: {
            name: dep.name,
            version: dep.version,
          },
          prefix: project.projectDir,
        })
        return
      }
      if ((await symlinkDependency(dep.depLocation, project.modulesDir, dep.alias)).reused) {
        return
      }
      rootLogger.debug({
        added: {
          dependencyType: dep.dependencyType,
          id: dep.id,
          // latest: opts.outdatedPkgs[pkg.id],
          name: dep.alias,
          realName: dep.name,
          version: dep.version,
        },
        prefix: project.projectDir,
      })
    }))
  }))
}
