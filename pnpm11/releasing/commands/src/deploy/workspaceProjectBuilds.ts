/**
 * `allowBuilds` approvals for the workspace projects a deploy installs as
 * `file:` dependencies. In the source workspace, a project runs its scripts
 * without approval. The deploy rewrites the project as a `file:` dependency,
 * which a name rule in `allowBuilds` does not approve.
 *
 * Each approval is the exact dependency path of a discovered workspace
 * project's root directory. A `file:` dependency on any other directory stays
 * unapproved. A denial in `allowBuilds` wins over the approval.
 */
import path from 'node:path'

import { createAllowBuildFunction, isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import * as dp from '@pnpm/deps.path'
import type { DirectoryResolution, PackageSnapshots } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'
import normalizePath from 'normalize-path'

import { createFileUrlDepPath, type DeployAllProjects } from './convertDeploySnapshots.js'

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
    packageSnapshots: PackageSnapshots
  }
): AllowBuilds | undefined {
  const projectDirs = projectRootDirs(opts.allProjects)
  const approved = new Set<string>()
  const rejected = new Set<string>()
  for (const depPath in opts.packageSnapshots) {
    const key = dp.getPkgIdWithPatchHash(depPath as DepPath)
    const resolution = opts.packageSnapshots[depPath as DepPath].resolution as Partial<DirectoryResolution>
    const directory = resolution.type === 'directory' && typeof resolution.directory === 'string'
      ? path.resolve(opts.deployDir, resolution.directory)
      : undefined
    const isProject = directory != null &&
      projectDirs.has(directory) &&
      createFileUrlDepPath({ resolvedPath: directory }, opts.allProjects) === dp.removeSuffix(depPath)
    if (isProject) {
      approved.add(key)
    } else {
      rejected.add(key)
    }
  }
  return approve(allowBuilds, Array.from(approved).filter(key => !rejected.has(key)))
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
  const keys = new Set<string>()
  for (const project of opts.allProjects) {
    if (!project.manifest.name) continue
    for (const rootDir of [project.rootDir, project.rootDirRealPath]) {
      keys.add(`${project.manifest.name}@file:${normalizePath(path.relative(opts.lockfileDir, rootDir))}`)
    }
  }
  return approve(allowBuilds, Array.from(keys))
}

function approve (allowBuilds: AllowBuilds | undefined, keys: string[]): AllowBuilds | undefined {
  const sourcePolicy = createAllowBuildFunction({ allowBuilds })
  const approvals = keys.filter(key => !isBuildExplicitlyDisallowed(key as DepPath, sourcePolicy))
  if (approvals.length === 0) return allowBuilds
  return {
    ...allowBuilds,
    ...Object.fromEntries(approvals.map(key => [key, true])),
  }
}

function projectRootDirs (allProjects: DeployAllProjects): Set<string> {
  return new Set(allProjects.flatMap(project => [project.rootDir, project.rootDirRealPath]))
}
