import fs from 'fs'
import path from 'path'
import { rootLogger } from '@pnpm/core-loggers'
import { symlinkDependency, symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import omit from 'ramda/src/omit'
import { readModulesDir } from '@pnpm/read-modules-dir'
import resolveLinkTarget from 'resolve-link-target'

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
  projects: Record<string, ProjectToLink>,
  opts: {
    dedupe: boolean
  }
) {
  if (opts.dedupe) {
    return linkDirectDepsAndDedupe(projects)
  }
  await Promise.all(Object.values(projects).map(linkDirectDepsOfProject))
}

async function linkDirectDepsAndDedupe (
  projects: Record<string, ProjectToLink>
) {
  let targetsInTheRoot!: string[]
  if (projects['.']) {
    await linkDirectDepsOfProject(projects['.'])
    if (Object.keys(projects).length === 1) return
    const pkgs = (await readModulesDir(projects['.'].modulesDir)) ?? []
    targetsInTheRoot = await Promise.all(pkgs.map((pkg) => resolveLinkTarget(path.join(projects['.'].modulesDir, pkg))))
  } else {
    targetsInTheRoot = []
  }
  await Promise.all(
    Object.values(omit(['.'], projects)).map(async (project) => {
      const pkgs = (await readModulesDir(project.modulesDir)) ?? []
      const targets = await Promise.all(pkgs.map(async (pkg) => {
        const location = path.join(project.modulesDir, pkg)
        return {
          location,
          realLocation: await resolveLinkTarget(location),
        }
      }))
      await Promise.all(targets
        .filter(({ realLocation }) => targetsInTheRoot.some((targetsInTheRoot) => path.relative(realLocation, targetsInTheRoot) === ''))
        .map(({ location }) => fs.promises.unlink(location))
      )
      return linkDirectDepsOfProject({
        ...project,
        dependencies: project.dependencies.filter((dep) => targetsInTheRoot.every((targetsInTheRoot) => path.relative(targetsInTheRoot, dep.dir) !== '')),
      })
    })
  )
}

async function linkDirectDepsOfProject (project: ProjectToLink) {
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
}
