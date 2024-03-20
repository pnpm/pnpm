import path from 'node:path'

import semver from 'semver'
import { prompt } from 'enquirer'
import realpathMissing from 'realpath-missing'

import {
  parseWantedDependency,
} from '@pnpm/parse-wanted-dependency'
import {
  readCurrentLockfile,
} from '@pnpm/lockfile-file'
import { PnpmError } from '@pnpm/error'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { isGitHostedPkgUrl } from '@pnpm/pick-fetcher'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import type { ParseWantedDependencyResult, TarballResolution, Config, GetPatchedDependencyOptions } from '@pnpm/types'

export async function getPatchedDependency(
  rawDependency: string,
  opts: GetPatchedDependencyOptions
): Promise<ParseWantedDependencyResult> {
  const dep = parseWantedDependency(rawDependency)

  const { versions, preferredVersions } = await getVersionsFromLockfile(
    dep,
    opts
  )

  if (!preferredVersions.length) {
    throw new PnpmError(
      'PATCH_VERSION_NOT_FOUND',
      `Can not find ${rawDependency} in project ${opts.lockfileDir}, ${versions.length ? `you can specify currently installed version: ${versions.map(({ version }) => version).join(', ')}.` : `did you forget to install ${rawDependency}?`}`
    )
  }

  dep.alias = dep.alias ?? rawDependency

  if (preferredVersions.length > 1) {
    const { version } = await prompt<{
      version: string
    }>({
      type: 'select',
      name: 'version',
      message: 'Choose which version to patch',
      choices: preferredVersions.map((preferred) => ({
        name: preferred.version,
        message: preferred.version,
        value: preferred.gitTarballUrl ?? preferred.version,
        hint: preferred.gitTarballUrl ? 'Git Hosted' : undefined,
      })),
      result(selected) {
        const selectedVersion = preferredVersions.find(
          (preferred) => preferred.version === selected
        )!
        return selectedVersion.gitTarballUrl ?? selected
      },
    })

    dep.pref = version
  } else {
    const preferred = preferredVersions[0]

    dep.pref = preferred.gitTarballUrl ?? preferred.version
  }

  return dep
}

export async function getVersionsFromLockfile(
  dep: ParseWantedDependencyResult,
  opts: GetPatchedDependencyOptions
) {
  const modulesDir = await realpathMissing(
    path.join(opts.lockfileDir, opts.modulesDir ?? 'node_modules')
  )
  const modules = await readModulesManifest(modulesDir)
  const lockfile =
    (modules?.virtualStoreDir &&
      (await readCurrentLockfile(modules.virtualStoreDir, {
        ignoreIncompatible: true,
      }))) ??
    null

  if (!lockfile) {
    throw new PnpmError(
      'PATCH_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot patch without a lockfile`
    )
  }

  const pkgName = dep.alias && dep.pref ? dep.alias : dep.pref ?? dep.alias

  const versions = Object.entries(lockfile.packages ?? {})
    .map(([depPath, pkgSnapshot]) => {
      const tarball =
        (pkgSnapshot.resolution as TarballResolution)?.tarball ?? ''
      return {
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
        gitTarballUrl: isGitHostedPkgUrl(tarball) ? tarball : undefined,
      }
    })
    .filter(({ name }) => name === pkgName)

  return {
    versions,
    preferredVersions: versions.filter(({ version }) =>
      dep.alias && dep.pref ? semver.satisfies(version, dep.pref) : true
    ),
  }
}
