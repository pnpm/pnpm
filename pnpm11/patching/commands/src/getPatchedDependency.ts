import path from 'node:path'

import { confirm, select } from '@inquirer/prompts'
import type { Config } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
import { type LockfileObject, type PackageSnapshots, readCurrentLockfile, type TarballResolution } from '@pnpm/lockfile.fs'
import { isGitHostedTarballUrl, nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { parseWantedDependency, type ParseWantedDependencyResult } from '@pnpm/resolving.parse-wanted-dependency'
import type { Project } from '@pnpm/types'
import { realpathMissing } from 'realpath-missing'
import semver from 'semver'

export type GetPatchedDependencyOptions = {
  lockfileDir: string
  workspaceDir?: string
  sharedWorkspaceLockfile?: boolean
  allProjects?: Array<Pick<Project, 'rootDir' | 'modulesDir'>>
} & Pick<Config, 'virtualStoreDir' | 'modulesDir'>

export type GetPatchedDependencyResult = ParseWantedDependencyResult & {
  applyToAll: boolean
  /**
   * The installed version of the chosen package. It differs from `bareSpecifier`
   * when the package is git-hosted, in which case `bareSpecifier` is its tarball URL.
   */
  version?: string
}

export async function getPatchedDependency (rawDependency: string, opts: GetPatchedDependencyOptions): Promise<GetPatchedDependencyResult> {
  const dep = parseWantedDependency(rawDependency)

  const { versions, preferredVersions } = await getVersionsFromLockfile(dep, opts)

  if (!preferredVersions.length) {
    throw new PnpmError(
      'PATCH_VERSION_NOT_FOUND',
      `Can not find ${rawDependency} in project ${opts.lockfileDir}, ${versions.length ? `you can specify currently installed version: ${versions.map(({ version }) => version).join(', ')}.` : `did you forget to install ${rawDependency}?`}`
    )
  }

  dep.alias = dep.alias ?? rawDependency
  if (preferredVersions.length > 1) {
    const { bareSpecifier, applyToAll } = await promptForVersionToPatch(preferredVersions)
    return {
      ...dep,
      applyToAll,
      bareSpecifier,
      version: preferredVersions.find(preferred => (preferred.gitTarballUrl ?? preferred.version) === bareSpecifier)?.version,
    }
  }
  const preferred = preferredVersions[0]
  if (preferred.gitTarballUrl) {
    return {
      ...dep,
      applyToAll: false,
      bareSpecifier: preferred.gitTarballUrl,
      version: preferred.version,
    }
  }
  return {
    ...dep,
    applyToAll: !dep.bareSpecifier,
    bareSpecifier: preferred.version,
    version: preferred.version,
  }
}

async function promptForVersionToPatch (preferredVersions: LockfileVersion[]): Promise<{ bareSpecifier: string, applyToAll: boolean }> {
  try {
    const bareSpecifier = await select({
      message: 'Choose which version to patch',
      choices: preferredVersions.map(preferred => ({
        name: preferred.version,
        value: preferred.gitTarballUrl ?? preferred.version,
        description: preferred.gitTarballUrl ? 'Git Hosted' : undefined,
      })),
      theme: { keybindings: ['vim'] },
    })
    const applyToAll = await confirm({
      message: 'Apply this patch to all versions?',
    })
    return { bareSpecifier, applyToAll }
  } catch (err: unknown) {
    if (isError(err) && err.name === 'ExitPromptError') {
      throw new PnpmError('PATCH_CANCELED', 'Canceled')
    }
    throw err
  }
}

// https://github.com/stackblitz-labs/pkg.pr.new
// When a package is installed via pkg.pr.new and has never been published to npm,
// the version or name obtained is incorrect, and an error will occur when patching. We can treat it as a tarball url.
export function isPkgPrNewUrl (url: string): boolean {
  return url.startsWith('https://pkg.pr.new/')
}

export interface LockfileVersion {
  gitTarballUrl?: string
  name: string
  peerDepGraphHash?: string
  version: string
}

export interface LockfileVersionsList {
  versions: LockfileVersion[]
  preferredVersions: LockfileVersion[]
}

export async function getVersionsFromLockfile (dep: ParseWantedDependencyResult, opts: GetPatchedDependencyOptions): Promise<LockfileVersionsList> {
  const lockfile = await readProjectCurrentLockfile(opts.lockfileDir, opts.modulesDir)
  const pkgName = dep.alias && dep.bareSpecifier ? dep.alias : (dep.bareSpecifier ?? dep.alias)

  if (lockfile != null) {
    const result = extractVersionsFromPackages(lockfile.packages ?? {}, dep, pkgName)
    if (result.preferredVersions.length > 0 || opts.sharedWorkspaceLockfile !== false || !opts.allProjects?.length) {
      return result
    }
  }

  const workspacePackages = await readWorkspaceCurrentPackages(opts, lockfile)
  if (workspacePackages == null) {
    throw new PnpmError(
      'PATCH_NO_LOCKFILE',
      'The modules directory is not ready for patching',
      {
        hint: 'Run pnpm install first',
      }
    )
  }

  return extractVersionsFromPackages(workspacePackages, dep, pkgName)
}

async function readProjectCurrentLockfile (rootDir: string, modulesDir?: string): Promise<LockfileObject | null> {
  const resolvedModulesDir = await realpathMissing(path.join(rootDir, modulesDir ?? 'node_modules'))
  return await readCurrentLockfile(path.join(resolvedModulesDir, '.pnpm'), {
    ignoreIncompatible: true,
  }) ?? null
}

async function readWorkspaceCurrentPackages (
  opts: GetPatchedDependencyOptions,
  activeLockfile: LockfileObject | null
): Promise<PackageSnapshots | null> {
  if (opts.sharedWorkspaceLockfile !== false || !opts.allProjects?.length) {
    return activeLockfile ? (activeLockfile.packages ?? {}) : null
  }
  const projectLockfiles = await Promise.all(
    opts.allProjects.map(async (project) => readProjectCurrentLockfile(project.rootDir, project.modulesDir ?? opts.modulesDir))
  )
  const foundLockfiles = [
    ...(activeLockfile ? [activeLockfile] : []),
    ...projectLockfiles.filter((lf): lf is LockfileObject => lf != null),
  ]
  if (foundLockfiles.length === 0) return null
  return Object.assign({}, ...foundLockfiles.map((lf) => lf.packages))
}

function extractVersionsFromPackages (
  packages: PackageSnapshots,
  dep: ParseWantedDependencyResult,
  pkgName: string | undefined
): LockfileVersionsList {
  const versions = Object.entries(packages)
    .map(([depPath, pkgSnapshot]) => {
      const tarball = (pkgSnapshot.resolution as TarballResolution)?.tarball ?? ''
      return {
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
        gitTarballUrl: (isGitHostedTarballUrl(tarball) || isPkgPrNewUrl(tarball)) ? tarball : undefined,
      }
    })
    .filter(({ name }) => name === pkgName)
    .sort((v1, v2) => semver.compare(v1.version, v2.version))

  return {
    versions,
    preferredVersions: versions.filter(({ version }) => dep.alias && dep.bareSpecifier ? semver.satisfies(version, dep.bareSpecifier) : true),
  }
}
