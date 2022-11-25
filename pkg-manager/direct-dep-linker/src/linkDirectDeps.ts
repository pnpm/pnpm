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
  if (opts.dedupe && projects['.'] && Object.keys(projects).length > 1) {
    return linkDirectDepsAndDedupe(projects['.'], omit(['.'], projects))
  }
  await Promise.all(Object.values(projects).map(linkDirectDepsOfProject))
}

async function linkDirectDepsAndDedupe (
  rootProject: ProjectToLink,
  projects: Record<string, ProjectToLink>
) {
  await linkDirectDepsOfProject(rootProject)
  const pkgsLinkedToRoot = await readLinkedDeps(rootProject.modulesDir)
  await Promise.all(
    Object.values(projects).map(async (project) => {
      await deletePkgsPresentInRoot(project.modulesDir, pkgsLinkedToRoot)
      return linkDirectDepsOfProject({
        ...project,
        dependencies: omitDepsFromRoot(project.dependencies, pkgsLinkedToRoot),
      })
    })
  )
}

function omitDepsFromRoot (deps: LinkedDirectDep[], pkgsLinkedToRoot: string[]) {
  return deps.filter((dep) =>
    pkgsLinkedToRoot.every(
      (pkgLinkedToRoot) => path.relative(pkgLinkedToRoot, dep.dir) !== ''
    )
  )
}

async function readLinkedDeps (modulesDir: string): Promise<string[]> {
  const deps = (await readModulesDir(modulesDir)) ?? []
  return Promise.all(
    deps.map((alias) => resolveLinkTarget(path.join(modulesDir, alias)))
  )
}

async function deletePkgsPresentInRoot (
  modulesDir: string,
  pkgsLinkedToRoot: string[]
) {
  const pkgsLinkedToCurrentProject = await readLinkedDepsWithRealLocations(modulesDir)
  await Promise.all(
    pkgsLinkedToCurrentProject
      .filter(({ linkedFrom }) => pkgsLinkedToRoot
        .some((pkgLinkedToRoot) => path.relative(linkedFrom, pkgLinkedToRoot) === ''))
      .map(({ linkedTo }) => fs.promises.unlink(linkedTo))
  )
}

async function readLinkedDepsWithRealLocations (modulesDir: string) {
  const deps = (await readModulesDir(modulesDir)) ?? []
  return Promise.all(deps.map(async (alias) => {
    const linkedTo = path.join(modulesDir, alias)
    return {
      linkedTo,
      linkedFrom: await resolveLinkTarget(linkedTo),
    }
  }))
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
