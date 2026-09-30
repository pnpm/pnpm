import fs from 'node:fs'
import path from 'node:path'

import { rootLogger } from '@pnpm/core-loggers'
import { isError } from '@pnpm/error'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { symlinkDependency, symlinkDirectRootDependency } from '@pnpm/fs.symlink-dependency'
import { rimraf } from '@zkochan/rimraf'
import { omit } from 'ramda'
import { resolveLinkTarget } from 'resolve-link-target'

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
  publishDir?: string
}

export async function linkDirectDeps (
  projects: Record<string, ProjectToLink>,
  opts: {
    dedupe: boolean
  }
): Promise<number> {
  if (opts.dedupe && projects['.'] && Object.keys(projects).length > 1) {
    return linkDirectDepsAndDedupe(projects['.'], omit(['.'], projects))
  }
  const numberOfLinkedDeps = await Promise.all(Object.values(projects).map(linkDirectDepsOfProject))
  return numberOfLinkedDeps.reduce((sum, count) => sum + count, 0)
}

async function linkDirectDepsAndDedupe (
  rootProject: ProjectToLink,
  projects: Record<string, ProjectToLink>
): Promise<number> {
  const linkedDeps = await linkDirectDepsOfProject(rootProject)
  const pkgsLinkedToRoot = await readLinkedDeps(rootProject.modulesDir)
  await Promise.all(
    Object.values(projects).map(async (project) => {
      const deletedAll = await deletePkgsPresentInRoot(project.modulesDir, pkgsLinkedToRoot)
      const dependencies = omitDepsFromRoot(project.dependencies, pkgsLinkedToRoot)
      if (dependencies.length > 0) {
        await linkDirectDepsOfProject({
          ...project,
          dependencies,
        })
        return
      }
      if (deletedAll) {
        await rimraf(project.modulesDir)
      }
      await removePublishModulesLink(project)
    })
  )
  return linkedDeps
}

function omitDepsFromRoot (deps: LinkedDirectDep[], pkgsLinkedToRoot: string[]): LinkedDirectDep[] {
  return deps.filter(({ dir }) => !pkgsLinkedToRoot.some(pathsEqual.bind(null, dir)))
}

function pathsEqual (path1: string, path2: string): boolean {
  return path.relative(path1, path2) === ''
}

async function readLinkedDeps (modulesDir: string): Promise<string[]> {
  const deps = (await readModulesDir(modulesDir)) ?? []
  return Promise.all(
    deps.map((alias) => resolveLinkTargetOrFile(path.join(modulesDir, alias)))
  )
}

async function deletePkgsPresentInRoot (
  modulesDir: string,
  pkgsLinkedToRoot: string[]
): Promise<boolean> {
  const pkgsLinkedToCurrentProject = await readLinkedDepsWithRealLocations(modulesDir)
  const pkgsToDelete = pkgsLinkedToCurrentProject
    .filter(({ linkedFrom, linkedTo }) => linkedFrom !== linkedTo && pkgsLinkedToRoot.some(pathsEqual.bind(null, linkedFrom)))
  await Promise.all(pkgsToDelete.map(({ linkedTo }) => fs.promises.unlink(linkedTo)))
  return pkgsToDelete.length === pkgsLinkedToCurrentProject.length
}

async function readLinkedDepsWithRealLocations (modulesDir: string) {
  const deps = (await readModulesDir(modulesDir)) ?? []
  return Promise.all(deps.map(async (alias) => {
    const linkedTo = path.join(modulesDir, alias)
    return {
      linkedTo,
      linkedFrom: await resolveLinkTargetOrFile(linkedTo),
    }
  }))
}

async function resolveLinkTargetOrFile (filePath: string): Promise<string> {
  try {
    return await resolveLinkTarget(filePath)
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || (err.code !== 'EINVAL' && err.code !== 'UNKNOWN')) throw err
    return filePath
  }
}

async function linkDirectDepsOfProject (project: ProjectToLink): Promise<number> {
  let linkedDeps = 0
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
    linkedDeps++
  }))
  await removePublishModulesLink(project)
  return linkedDeps
}

/**
 * Removes `<publishDir>/node_modules` when it is a link that resolves to the
 * project's modules directory, as pnpm 11.28.0 created for
 * `publishConfig.linkDirectory`. A build tool that cleans the publish
 * directory through that link deletes the dependencies' files. Real
 * directories and links to anything else are left alone.
 */
async function removePublishModulesLink (project: ProjectToLink): Promise<void> {
  if (!project.publishDir) return
  const projectDir = path.resolve(project.dir)
  const publishDir = path.resolve(projectDir, project.publishDir)
  if (!isSubdirectory(projectDir, publishDir)) return
  const link = path.join(publishDir, path.basename(project.modulesDir))
  const stats = await tryLstat(link)
  if (!stats?.isSymbolicLink()) return
  const [linkTarget, modulesDir] = await Promise.all([safeRealpath(link), safeRealpath(project.modulesDir)])
  if (linkTarget == null || linkTarget !== modulesDir) return
  await rimraf(link)
}

function isSubdirectory (parent: string, child: string): boolean {
  const relative = path.relative(parent, child)
  return Boolean(relative && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative))
}

async function tryLstat (target: string): Promise<fs.Stats | undefined> {
  try {
    return await fs.promises.lstat(target)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return undefined
    throw err
  }
}

async function safeRealpath (target: string): Promise<string | null> {
  try {
    return await fs.promises.realpath(target)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return null
    throw err
  }
}
