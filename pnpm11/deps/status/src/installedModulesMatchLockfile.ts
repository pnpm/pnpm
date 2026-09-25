import path from 'node:path'

import {
  getLockfileImporterId,
  type LockfileObject,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile.fs'
import { satisfiesPackageManifest } from '@pnpm/lockfile.verification'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

import { assertLockfilesEqual } from './assertLockfilesEqual.js'
import type { CheckDepsStatusOptions } from './checkDepsStatus.js'

/**
 * Whether `node_modules` already contains what the wanted lockfile describes.
 *
 * The workspace state file is how `pnpm run` usually knows that. When that
 * file is missing or unreadable, the gate would spawn an install, which opens
 * the store index and contacts the registry. This check uses the lockfiles
 * and the project manifests only, so a script can run with a read-only store
 * and no network.
 *
 * Returns `false` when the tree cannot be proved installed, including when a
 * lockfile cannot be read. The caller then keeps its existing out-of-date
 * behavior.
 */
export async function installedModulesMatchLockfile (opts: CheckDepsStatusOptions): Promise<boolean> {
  const lockfileDir = opts.lockfileDir ?? opts.workspaceDir ?? opts.rootProjectManifestDir
  if (lockfileDir == null) return false
  try {
    const wantedLockfile = await readWantedLockfile(lockfileDir, {
      ignoreIncompatible: false,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
      useGitBranchLockfile: opts.useGitBranchLockfile,
    })
    if (wantedLockfile == null) return false
    const modulesDir = path.resolve(lockfileDir, opts.modulesDir ?? 'node_modules')
    const currentLockfile = await readCurrentLockfile(path.join(modulesDir, '.pnpm'), { ignoreIncompatible: false })
    assertLockfilesEqual(currentLockfile, wantedLockfile, lockfileDir)
    return await manifestsSatisfyLockfile(opts, lockfileDir, wantedLockfile)
  } catch {
    return false
  }
}

async function manifestsSatisfyLockfile (opts: CheckDepsStatusOptions, lockfileDir: string, wantedLockfile: LockfileObject): Promise<boolean> {
  const projects = await projectsToCheck(opts)
  if (projects == null || projects.length === 0) return false
  for (const project of projects) {
    const projectId: ProjectId = opts.sharedWorkspaceLockfile === false
      ? '.' as ProjectId
      : getLockfileImporterId(lockfileDir, project.rootDir)
    if (!satisfiesPackageManifest({
      autoInstallPeers: opts.autoInstallPeers,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
    }, wantedLockfile.importers[projectId], project.manifest).satisfies) {
      return false
    }
  }
  return true
}

async function projectsToCheck (opts: CheckDepsStatusOptions): Promise<Array<{ rootDir: string, manifest: ProjectManifest }> | undefined> {
  if (opts.allProjects != null && opts.allProjects.length > 0) {
    return opts.allProjects.map(({ manifest, rootDir }) => ({ manifest, rootDir }))
  }
  const discovered = await discoverWorkspaceProjects(opts)
  if (discovered === 'unreadable') return undefined
  if (discovered != null && discovered.length > 0) return discovered
  if (opts.rootProjectManifest != null && opts.rootProjectManifestDir != null) {
    return [{ manifest: opts.rootProjectManifest, rootDir: opts.rootProjectManifestDir }]
  }
  return discovered ?? []
}

async function discoverWorkspaceProjects (opts: CheckDepsStatusOptions): Promise<Array<{ rootDir: string, manifest: ProjectManifest }> | 'unreadable' | undefined> {
  const workspaceRoot = opts.workspaceDir ?? opts.rootProjectManifestDir
  if (workspaceRoot == null || opts.rootProjectManifestDir == null) return undefined
  try {
    const workspaceManifest = await readWorkspaceManifest(workspaceRoot)
    if (workspaceManifest == null && opts.workspaceDir == null) return undefined
    const allProjects = await findWorkspaceProjectsNoCheck(opts.rootProjectManifestDir, {
      patterns: workspaceManifest == null ? undefined : workspaceManifest.packages ?? ['.'],
      modulesDir: opts.modulesDir,
      modulesDirsByProjectName: opts.modulesDirsByProjectName,
    })
    return allProjects.map(({ manifest, rootDir }) => ({ manifest, rootDir }))
  } catch {
    return 'unreadable'
  }
}
