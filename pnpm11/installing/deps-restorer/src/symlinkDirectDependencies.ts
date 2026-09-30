import path from 'node:path'

import { packageManifestLogger } from '@pnpm/core-loggers'
import type { DirectDependenciesByImporterId } from '@pnpm/deps.graph-builder'
import * as dp from '@pnpm/deps.path'
import { linkDirectDeps, type LinkedDirectDep } from '@pnpm/installing.linking.direct-dep-linker'
import { getLockfileImporterId, type LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type {
  DependencyManifest,
  ProjectId,
  ProjectManifest,
  RegistriesByScope,
} from '@pnpm/types'
import { readProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { omit } from 'ramda'

import type { HeadlessOptions, Project } from './types.js'

type SymlinkDirectDependenciesOpts = Pick<HeadlessOptions, 'registriesByScope' | 'symlink' | 'lockfileDir'> & {
  filteredLockfile: LockfileObject
  dedupe: boolean
  directDependenciesByImporterId: DirectDependenciesByImporterId
  projects: Project[]
}

export async function symlinkDirectDependencies (
  {
    filteredLockfile,
    dedupe,
    directDependenciesByImporterId,
    lockfileDir,
    projects,
    registriesByScope,
    symlink,
  }: SymlinkDirectDependenciesOpts
): Promise<number> {
  for (const { rootDir, manifest } of projects) {
    // Even though headless installation will never update the package.json
    // this needs to be logged because otherwise install summary won't be printed
    packageManifestLogger.debug({
      prefix: rootDir,
      updated: manifest,
    })
  }
  if (symlink === false) return 0
  const importerManifestsByImporterId = {} as { [id: string]: ProjectManifest }
  for (const { id, manifest } of projects) {
    importerManifestsByImporterId[id] = manifest
  }
  const projectsToLink = Object.fromEntries(await Promise.all(
    projects.map(async ({ rootDir, id, modulesDir }) => ([id, {
      dir: rootDir,
      modulesDir,
      publishDir: filteredLockfile.importers[id]?.publishDirectory,
      dependencies: await getRootPackagesToLink(filteredLockfile, {
        importerId: id,
        importerModulesDir: modulesDir,
        lockfileDir,
        projectDir: rootDir,
        importerManifestsByImporterId,
        registriesByScope,
        rootDependencies: directDependenciesByImporterId[id],
      }),
    }]))
  ))
  const rootProject = projectsToLink['.']
  if (rootProject && dedupe) {
    const rootDeps = Object.fromEntries(rootProject.dependencies.map((dep: LinkedDirectDep) => [dep.alias, dep.dir]))
    for (const project of Object.values(omit(['.'], projectsToLink))) {
      project.dependencies = project.dependencies.filter((dep: LinkedDirectDep) => dep.dir !== rootDeps[dep.alias])
    }
  }
  return linkDirectDeps(projectsToLink, { dedupe: Boolean(dedupe) })
}

interface RootPackagesToLinkOptions {
  registriesByScope: RegistriesByScope
  projectDir: string
  importerId: ProjectId
  importerModulesDir: string
  importerManifestsByImporterId: { [id: string]: ProjectManifest }
  lockfileDir: string
  rootDependencies: { [alias: string]: string }
}

type ProjectSnapshot = LockfileObject['importers'][ProjectId]

async function getRootPackagesToLink (
  lockfile: LockfileObject,
  opts: RootPackagesToLinkOptions
): Promise<LinkedDirectDep[]> {
  const projectSnapshot = lockfile.importers[opts.importerId]
  const allDeps = {
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.dependencies,
    ...projectSnapshot.optionalDependencies,
  }
  return (await Promise.all(
    Object.entries(allDeps)
      .map(async ([alias, ref]) => {
        const dependencyType = getDependencyType(projectSnapshot, alias)
        if (ref.startsWith('link:')) {
          return getExternalLinkToLink({ alias, dependencyType, linkRef: ref }, opts)
        }
        return getLockedPackageToLink({ alias, dependencyType, ref }, { lockfile, rootDependencies: opts.rootDependencies })
      })
  ))
    .filter(Boolean) as LinkedDirectDep[]
}

function getDependencyType (projectSnapshot: ProjectSnapshot, alias: string): LinkedDirectDep['dependencyType'] {
  if (projectSnapshot.devDependencies?.[alias]) return 'dev'
  if (projectSnapshot.optionalDependencies?.[alias]) return 'optional'
  return 'prod'
}

async function getExternalLinkToLink (
  { alias, dependencyType, linkRef }: { alias: string, dependencyType: LinkedDirectDep['dependencyType'], linkRef: string },
  opts: RootPackagesToLinkOptions
): Promise<LinkedDirectDep> {
  const ref = linkRef.slice(5)
  const packageDir = path.isAbsolute(ref) ? ref : path.join(opts.projectDir, ref)
  const linkedPackage = await readLinkedPackageManifest(alias, packageDir, opts)
  return {
    alias,
    name: linkedPackage.name,
    version: linkedPackage.version,
    dir: packageDir,
    id: ref,
    isExternalLink: true,
    dependencyType,
  }
}

async function readLinkedPackageManifest (
  alias: string,
  packageDir: string,
  opts: Pick<RootPackagesToLinkOptions, 'importerManifestsByImporterId' | 'lockfileDir'>
): Promise<DependencyManifest> {
  const importerId = getLockfileImporterId(opts.lockfileDir, packageDir)
  if (opts.importerManifestsByImporterId[importerId]) {
    return opts.importerManifestsByImporterId[importerId] as DependencyManifest
  }
  try {
    // TODO: cover this case with a test
    return await readProjectManifestOnly(packageDir) as DependencyManifest
  } catch (err: unknown) {
    if ((err as { code?: unknown })['code'] !== 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') throw err
    return { name: alias, version: '0.0.0' }
  }
}

function getLockedPackageToLink (
  { alias, dependencyType, ref }: { alias: string, dependencyType: LinkedDirectDep['dependencyType'], ref: string },
  { lockfile, rootDependencies }: { lockfile: LockfileObject, rootDependencies: RootPackagesToLinkOptions['rootDependencies'] }
): LinkedDirectDep | undefined {
  const dir = rootDependencies[alias]
  // Skipping linked packages
  if (!dir) {
    return undefined
  }
  const depPath = dp.refToRelative(ref, alias)
  if (depPath === null) return undefined
  const pkgSnapshot = lockfile.packages?.[depPath]
  if (pkgSnapshot == null) return undefined // this won't ever happen. Just making typescript happy
  const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  return {
    alias,
    isExternalLink: false,
    name: pkgInfo.name,
    version: pkgInfo.version,
    dependencyType,
    dir,
    id: pkgSnapshot.id ?? depPath,
  }
}
