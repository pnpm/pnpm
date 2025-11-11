import path from 'path'
import { parseWantedDependency, type ParseWantedDependencyResult } from '@pnpm/parse-wanted-dependency'
import enquirer from 'enquirer'
import { readCurrentLockfile, type TarballResolution } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { PnpmError } from '@pnpm/error'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import realpathMissing from 'realpath-missing'
import semver from 'semver'
import { type Config } from '@pnpm/config'

export type GetPatchedDependencyOptions = {
  lockfileDir: string
} & Pick<Config, 'virtualStoreDir' | 'modulesDir'>

export type GetPatchedDependencyResult = ParseWantedDependencyResult & { applyToAll: boolean }

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
    const { version, applyToAll } = await enquirer.prompt<{
      version: string
      applyToAll: boolean
    }>([{
      type: 'select',
      name: 'version',
      message: 'Choose which version to patch',
      choices: preferredVersions.map(preferred => ({
        name: preferred.version,
        message: preferred.version,
        value: preferred.gitTarballUrl ?? preferred.version,
        hint: preferred.gitTarballUrl ? 'Git Hosted' : undefined,
      })),
      result (selected) {
        const selectedVersion = preferredVersions.find(preferred => preferred.version === selected)!
        return selectedVersion.gitTarballUrl ?? selected
      },
    }, {
      type: 'confirm',
      name: 'applyToAll',
      message: 'Apply this patch to all versions?',
    }])
    return {
      ...dep,
      applyToAll,
      bareSpecifier: version,
    }
  } else {
    const preferred = preferredVersions[0]
    if (preferred.gitTarballUrl) {
      return {
        ...opts,
        applyToAll: false,
        bareSpecifier: preferred.gitTarballUrl,
      }
    }
    return {
      ...dep,
      applyToAll: !dep.bareSpecifier,
      bareSpecifier: preferred.version,
    }
  }
}

// https://github.com/stackblitz-labs/pkg.pr.new
// With pkg.pr.new, each of your commits and pull requests will trigger an instant preview release without publishing anything to NPM.
// This enables users to access features and bug-fixes without the need to wait for release cycles using npm or pull request merges.
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
  const modulesDir = await realpathMissing(path.join(opts.lockfileDir, opts.modulesDir ?? 'node_modules'))
  const lockfile = await readCurrentLockfile(path.join(modulesDir, '.pnpm'), {
    ignoreIncompatible: true,
  }) ?? null

  if (!lockfile) {
    throw new PnpmError(
      'PATCH_NO_LOCKFILE',
      'The modules directory is not ready for patching',
      {
        hint: 'Run pnpm install first',
      }
    )
  }

  const pkgName = dep.alias && dep.bareSpecifier ? dep.alias : (dep.bareSpecifier ?? dep.alias)

  const versions = Object.entries(lockfile.packages ?? {})
    .map(([depPath, pkgSnapshot]) => {
      const tarball = (pkgSnapshot.resolution as TarballResolution)?.tarball ?? ''
      return {
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
        gitTarballUrl: (isGitHostedPkgUrl(tarball) || isPkgPrNewUrl(tarball)) ? tarball : undefined,
      }
    })
    .filter(({ name }) => name === pkgName)
    .sort((v1, v2) => semver.compare(v1.version, v2.version))

  return {
    versions,
    preferredVersions: versions.filter(({ version }) => dep.alias && dep.bareSpecifier ? semver.satisfies(version, dep.bareSpecifier) : true),
  }
}
