/**
 * `allowBuilds` approvals for the workspace projects a deploy installs as
 * `file:` dependencies. In the source workspace, a project runs its scripts
 * without approval. The deploy rewrites the project as a `file:` dependency,
 * which a name rule in `allowBuilds` does not approve.
 *
 * Each approval is the exact dependency path of a discovered workspace
 * project's root directory, or of the publish directory that dependents link
 * it from. A publish directory qualifies only when it is a real directory
 * inside the project root and outside `node_modules`. A `file:` dependency on
 * any other directory stays unapproved. A denial in `allowBuilds` wins over
 * the approval, whether it names the project's deployed key or a key the
 * source workspace gives it.
 */
import fs from 'node:fs'
import path from 'node:path'

import { createAllowBuildFunction, isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import * as dp from '@pnpm/deps.path'
import { isError } from '@pnpm/error'
import type { DirectoryResolution, PackageSnapshots } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'
import normalizePath from 'normalize-path'

import { createFileUrlDepPath, type DeployAllProjects, getLinkedPublishDir } from './convertDeploySnapshots.js'

type AllowBuilds = Record<string, boolean | string>

/**
 * Approves each workspace project in the lockfile of a shared-lockfile deploy.
 * A package qualifies only when its directory resolution points at a project
 * root and its key is the one the deploy generates for that root. Every
 * snapshot of a package shares one approval, so one snapshot that fails the
 * check leaves the package unapproved.
 */
export function approveWorkspaceProjectBuilds (
  allowBuilds: AllowBuilds | undefined,
  opts: {
    allProjects: DeployAllProjects
    deployDir: string
    /** The lockfile dir of the source workspace. */
    lockfileDir: string
    packageSnapshots: PackageSnapshots
  }
): AllowBuilds | undefined {
  const projectsByDir = indexProjectsByRootDir(opts.allProjects)
  const approved = new Map<string, ProjectBuildKey>()
  const rejected = new Set<string>()
  for (const depPath in opts.packageSnapshots) {
    const key = dp.getPkgIdWithPatchHash(depPath as DepPath)
    const resolution = opts.packageSnapshots[depPath as DepPath].resolution as Partial<DirectoryResolution>
    const directory = resolution.type === 'directory' && typeof resolution.directory === 'string'
      ? path.resolve(opts.deployDir, resolution.directory)
      : undefined
    const project = directory == null ? undefined : projectsByDir.get(directory)
    if (
      directory != null &&
      project != null &&
      createFileUrlDepPath({ resolvedPath: directory }, opts.allProjects) === dp.removeSuffix(depPath)
    ) {
      approved.set(key, { key, sourceKeys: sourceProjectKeys(project, opts.lockfileDir) })
    } else {
      rejected.add(key)
    }
  }
  return approve(allowBuilds, Array.from(approved.values()).filter(({ key }) => !rejected.has(key)))
}

/**
 * Approves each workspace project under the dependency path the resolver of a
 * legacy deploy gives an injected project: its root relative to `lockfileDir`.
 */
export function approveLegacyWorkspaceProjectBuilds (
  allowBuilds: AllowBuilds | undefined,
  opts: {
    allProjects: DeployAllProjects
    lockfileDir: string
  }
): AllowBuilds | undefined {
  const keys = opts.allProjects.flatMap(project => {
    const sourceKeys = sourceProjectKeys(project, opts.lockfileDir)
    return sourceKeys.map(key => ({ key, sourceKeys }))
  })
  return approve(allowBuilds, keys)
}

/** A key to approve, and the source workspace keys of the same project, a denial of which also applies to it. */
interface ProjectBuildKey {
  key: string
  sourceKeys: string[]
}

function approve (allowBuilds: AllowBuilds | undefined, keys: ProjectBuildKey[]): AllowBuilds | undefined {
  const sourcePolicy = createAllowBuildFunction({ allowBuilds })
  const isDenied = (key: string) => isBuildExplicitlyDisallowed(key as DepPath, sourcePolicy)
  const approvals = keys
    .filter(({ key, sourceKeys }) => !isDenied(key) && !sourceKeys.some(isDenied))
    .map(({ key }) => key)
  if (approvals.length === 0) return allowBuilds
  return {
    ...allowBuilds,
    ...Object.fromEntries(approvals.map(key => [key, true])),
  }
}

/** The keys an install in `lockfileDir` gives the project as an injected `file:` dependency. */
function sourceProjectKeys (project: DeployAllProjects[number], lockfileDir: string): string[] {
  if (!project.manifest.name) return []
  return projectPackageDirs(project)
    .map(packageDir => `${project.manifest.name}@file:${normalizePath(path.relative(lockfileDir, packageDir))}`)
}

function indexProjectsByRootDir (allProjects: DeployAllProjects): Map<string, DeployAllProjects[number]> {
  return new Map(allProjects.flatMap(project => projectPackageDirs(project).map(packageDir => [packageDir, project])))
}

/** Every spelling of the directories a dependent may install the project from. */
function projectPackageDirs (project: DeployAllProjects[number]): string[] {
  const packageDirs: string[] = [project.rootDir, project.rootDirRealPath]
  const publishDir = getLinkedPublishDir(project)
  if (publishDir != null && isOwnPublishDir(project.rootDirRealPath, publishDir)) {
    packageDirs.push(path.join(project.rootDir, path.relative(project.rootDirRealPath, publishDir)), publishDir)
  }
  return packageDirs
}

/**
 * Whether `publishDir` is a real directory inside the project root, reached
 * through no symlink and no `node_modules`. `node_modules` matches in any
 * case, as a case-insensitive file system resolves every spelling to it.
 */
function isOwnPublishDir (rootDirRealPath: string, publishDir: string): boolean {
  const relativePath = path.relative(rootDirRealPath, publishDir)
  if (relativePath === '' || path.isAbsolute(relativePath)) return false
  const segments = relativePath.split(path.sep)
  if (segments[0] === '..' || segments.some(segment => segment.toLowerCase() === 'node_modules')) return false
  return realPathOrUndefined(publishDir) === publishDir
}

function realPathOrUndefined (dir: string): string | undefined {
  try {
    return fs.realpathSync(dir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return undefined
    throw err
  }
}
