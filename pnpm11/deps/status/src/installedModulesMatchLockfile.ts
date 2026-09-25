import path from 'node:path'

import { parseOverrides } from '@pnpm/config.parse-overrides'
import { createProjectModulesDirResolver } from '@pnpm/config.reader'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import {
  checkPatchedDepPaths,
  getLockfileImporterId,
  type LockfileObject,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile.fs'
import {
  calcPatchHashes,
  createOverridesMapFromParsed,
  DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
  getOutdatedLockfileSetting,
  resolvePatchedDependencies,
} from '@pnpm/lockfile.settings-checker'
import { satisfiesPackageManifest } from '@pnpm/lockfile.verification'
import type { ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { isEmpty } from 'ramda'

import { assertLockfilesEqual } from './assertLockfilesEqual.js'
import type { CheckDepsStatusOptions } from './checkDepsStatus.js'
import { dedupeLinksNothing } from './dedupeLinksNothing.js'
import { safeStat } from './safeStat.js'

interface ProjectToCheck {
  manifest: ProjectManifest
  rootDir: ProjectRootDir
}

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
  try {
    const projects = await projectsToCheck(opts)
    if (projects == null || projects.length === 0) return false

    if (opts.sharedWorkspaceLockfile === false) {
      return await separateLockfilesMatch(projects, opts)
    }

    const lockfileDir = opts.lockfileDir ?? opts.workspaceDir ?? opts.rootProjectManifestDir
    if (lockfileDir == null) return false

    const wantedLockfile = await readWantedLockfile(lockfileDir, {
      ignoreIncompatible: false,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
      useGitBranchLockfile: opts.useGitBranchLockfile,
    })
    if (wantedLockfile == null) return false

    const modulesDir = path.resolve(lockfileDir, opts.modulesDir ?? 'node_modules')
    const currentLockfile = await readCurrentLockfile(path.join(modulesDir, '.pnpm'), { ignoreIncompatible: false })
    assertLockfilesEqual(currentLockfile, wantedLockfile, lockfileDir)

    if (!await lockfileSettingsUpToDate(wantedLockfile, lockfileDir, opts)) return false
    if (!await checkProjectsHaveModulesDir(projects, opts, lockfileDir, wantedLockfile)) return false

    for (const project of projects) {
      const projectId: ProjectId = getLockfileImporterId(lockfileDir, project.rootDir)
      const importer = wantedLockfile.importers[projectId]
      if (importer == null) return false
      if (!satisfiesPackageManifest({
        autoInstallPeers: opts.autoInstallPeers,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
      }, importer, project.manifest).satisfies) {
        return false
      }
    }
    return true
  } catch {
    return false
  }
}

async function separateLockfilesMatch (
  projects: ProjectToCheck[],
  opts: CheckDepsStatusOptions
): Promise<boolean> {
  const matches = await Promise.all(projects.map(async project => projectLockfileMatches(project, opts)))
  return matches.every(Boolean)
}

async function projectLockfileMatches (
  project: ProjectToCheck,
  opts: CheckDepsStatusOptions
): Promise<boolean> {
  const wantedLockfile = await readWantedLockfile(project.rootDir, {
    ignoreIncompatible: false,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    useGitBranchLockfile: opts.useGitBranchLockfile,
  })
  if (wantedLockfile == null) return false

  const modulesDir = path.resolve(project.rootDir, opts.modulesDir ?? 'node_modules')
  const currentLockfile = await readCurrentLockfile(path.join(modulesDir, '.pnpm'), { ignoreIncompatible: false })
  assertLockfilesEqual(currentLockfile, wantedLockfile, project.rootDir)

  if (!await lockfileSettingsUpToDate(wantedLockfile, project.rootDir, opts)) return false
  if (!await checkProjectsHaveModulesDir([project], opts, project.rootDir, wantedLockfile)) return false

  const importer = wantedLockfile.importers['.' as ProjectId]
  if (importer == null) return false
  return satisfiesPackageManifest({
    autoInstallPeers: opts.autoInstallPeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
  }, importer, project.manifest).satisfies
}

async function lockfileSettingsUpToDate (
  wantedLockfile: LockfileObject,
  lockfileDir: string,
  opts: CheckDepsStatusOptions
): Promise<boolean> {
  const resolvedPatchedDeps = resolvePatchedDependencies(opts.patchedDependencies, lockfileDir)
  const [
    patchedDependencies,
    pnpmfileChecksum,
  ] = await Promise.all([
    calcPatchHashes(resolvedPatchedDeps ?? {}),
    opts.hooks?.calculatePnpmfileChecksum?.(),
  ])

  const outdatedLockfileSettingName = getOutdatedLockfileSetting(wantedLockfile, {
    catalogs: opts.catalogs,
    autoInstallPeers: opts.autoInstallPeers,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    peersSuffixMaxLength: opts.peersSuffixMaxLength ?? DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
    overrides: createOverridesMapFromParsed(parseOverrides(opts.overrides ?? {}, opts.catalogs)),
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies == null ? undefined : [...opts.ignoredOptionalDependencies].sort(),
    packageExtensionsChecksum: hashObjectNullableWithPrefix(opts.packageExtensions),
    patchedDependencies,
    pnpmfileChecksum,
    ignorePnpmfileChecksum: opts.ignorePnpmfile === true && pnpmfileChecksum == null,
  })

  if (outdatedLockfileSettingName != null) return false
  return checkPatchedDepPaths(wantedLockfile) === 'up-to-date'
}

async function checkProjectsHaveModulesDir (
  projects: ProjectToCheck[],
  opts: CheckDepsStatusOptions,
  lockfileDir: string,
  wantedLockfile: LockfileObject
): Promise<boolean> {
  const rootProjectDir = opts.rootProjectManifestDir ?? lockfileDir
  const modulesDirOf = createProjectModulesDirResolver(opts)

  const statModulesDir = (project: ProjectToCheck) => {
    if (opts.nodeLinker === 'hoisted') {
      return safeStat(path.resolve(rootProjectDir, opts.modulesDir ?? 'node_modules'))
    }
    return safeStat(path.resolve(project.rootDir, modulesDirOf(project.manifest.name) ?? 'node_modules'))
  }

  const allManifestStats = await Promise.all(projects.map(async project => ({
    project,
    modulesDirStats: await statModulesDir(project),
  })))

  const withoutModulesDir = allManifestStats.filter(({ modulesDirStats, project }) =>
    modulesDirStats?.isDirectory() !== true && !isEmpty({
      ...project.manifest.dependencies,
      ...project.manifest.devDependencies,
    }))

  if (withoutModulesDir.length === 0) return true

  const rootModulesDirExists = allManifestStats.some(({ modulesDirStats, project }) =>
    modulesDirStats?.isDirectory() === true && project.rootDir === rootProjectDir)

  const dedupeLockfileDir = opts.lockfileDir ?? opts.workspaceDir ?? opts.rootProjectManifestDir ?? lockfileDir
  const mayBeDeduped = (project: ProjectToCheck): boolean =>
    opts.dedupeDirectDeps === true && rootModulesDirExists && project.rootDir !== rootProjectDir

  for (const { project } of withoutModulesDir) {
    if (
      mayBeDeduped(project) &&
      dedupeLinksNothing(wantedLockfile, dedupeLockfileDir, rootProjectDir, project.rootDir, opts.include)
    ) continue
    return false
  }

  return true
}

async function projectsToCheck (opts: CheckDepsStatusOptions): Promise<ProjectToCheck[] | undefined> {
  if (opts.allProjects != null && opts.allProjects.length > 0) {
    const projects: ProjectToCheck[] = opts.allProjects.map(({ manifest, rootDir }) => ({ manifest, rootDir }))
    if (
      opts.rootProjectManifest != null &&
      opts.rootProjectManifestDir != null &&
      !opts.allProjects.some(({ rootDir }) => rootDir === opts.rootProjectManifestDir)
    ) {
      projects.push({ manifest: opts.rootProjectManifest, rootDir: opts.rootProjectManifestDir as ProjectRootDir })
    }
    return projects
  }
  if (opts.sharedWorkspaceLockfile === false && opts.rootProjectManifest != null && opts.rootProjectManifestDir != null) {
    return [{ manifest: opts.rootProjectManifest, rootDir: opts.rootProjectManifestDir as ProjectRootDir }]
  }
  const discovered = await discoverWorkspaceProjects(opts)
  if (discovered === 'unreadable') return undefined
  if (discovered != null && discovered.length > 0) return discovered
  if (opts.rootProjectManifest != null && opts.rootProjectManifestDir != null) {
    return [{ manifest: opts.rootProjectManifest, rootDir: opts.rootProjectManifestDir as ProjectRootDir }]
  }
  return discovered ?? []
}

async function discoverWorkspaceProjects (opts: CheckDepsStatusOptions): Promise<ProjectToCheck[] | 'unreadable' | undefined> {
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
