import path from 'node:path'

import { readProjectManifestOnly } from '@pnpm/cli.utils'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { findDependencyLicenses, type LicensePackage } from '@pnpm/deps.compliance.license-scanner'
import { PnpmError } from '@pnpm/error'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import { getStorePath } from '@pnpm/store.path'
import type { ProjectId } from '@pnpm/types'
import semver from 'semver'

import type { LicensesCommandResult } from './LicensesCommandResult.js'
import { renderLicences } from './outputRenderer.js'

export type LicensesCommandOptions = {
  compatible?: boolean
  long?: boolean
  recursive?: boolean
  json?: boolean
} & Pick<
  Config,
| 'dev'
| 'dir'
| 'lockfileDir'
| 'registriesByScope'
| 'registriesByPrefix'
| 'optional'
| 'production'
| 'resolvePeersFromWorkspaceRoot'
| 'sharedWorkspaceLockfile'
| 'storeDir'
| 'virtualStoreDir'
| 'modulesDir'
| 'nodeLinker'
| 'pnpmHomeDir'
| 'supportedArchitectures'
| 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'selectedProjectsGraph'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> &
Partial<Pick<Config, 'userConfig'>>

export async function licensesList (opts: LicensesCommandOptions): Promise<LicensesCommandResult> {
  const lockfiles = await Promise.all(
    Array.from(importerIdsByLockfileDir(opts), async ([lockfileDir, includedImporterIds]) => {
      const lockfile = await readWantedLockfile(lockfileDir, {
        ignoreIncompatible: true,
      })
      if (lockfile == null) {
        throw new PnpmError(
          'LICENSES_NO_LOCKFILE',
          `No ${WANTED_LOCKFILE} found in "${lockfileDir}": Cannot check a project without a lockfile`
        )
      }
      return { lockfileDir, lockfile, includedImporterIds }
    })
  )

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  const manifest = await readProjectManifestOnly(opts.dir)

  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })

  const licensePackagesByLockfile = await Promise.all(
    lockfiles.map(async ({ lockfileDir, lockfile, includedImporterIds }) => {
      const modules = opts.nodeLinker === 'hoisted'
        ? await readModulesManifest(path.resolve(lockfileDir, opts.modulesDir ?? 'node_modules'))
        : null
      return findDependencyLicenses({
        include,
        lockfileDir,
        storeDir,
        virtualStoreDir: opts.virtualStoreDir ?? path.join(opts.modulesDir ?? 'node_modules', '.pnpm'),
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
        modulesDir: opts.modulesDir,
        hoistedLocations: modules?.hoistedLocations,
        registriesByScope: opts.registriesByScope,
        registriesByPrefix: opts.registriesByPrefix,
        wantedLockfile: lockfile,
        manifest,
        includedImporterIds,
        resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
        supportedArchitectures: opts.supportedArchitectures,
      })
    })
  )
  // A package installed by several projects is listed once. Two local
  // packages can share a name and version but not their license.
  const licensePackages = new Map<string, LicensePackage>()
  for (const licensePackage of licensePackagesByLockfile.flat()) {
    const key = `${licensePackage.name}@${licensePackage.registryName ?? ''}:${licensePackage.version}\u0000${licensePackage.license}`
    if (!licensePackages.has(key)) {
      licensePackages.set(key, licensePackage)
    }
  }

  if (licensePackages.size === 0)
    return { output: 'No licenses in packages found', exitCode: 0 }

  const sortedLicensePackages = Array.from(licensePackages.values()).sort((pkg1, pkg2) =>
    pkg1.name.localeCompare(pkg2.name) || compareVersions(pkg1.version, pkg2.version)
  )
  return renderLicences(sortedLicensePackages, opts)
}

/**
 * A local package's lockfile entry may carry no version, which
 * `semver.compare` rejects.
 */
function compareVersions (version1: string | undefined, version2: string | undefined): number {
  const valid1 = semver.valid(version1)
  const valid2 = semver.valid(version2)
  if (valid1 != null && valid2 != null) return semver.compare(valid1, valid2)
  return (version1 ?? '').localeCompare(version2 ?? '')
}

/**
 * The selected importers grouped by the lockfile that records them: one
 * lockfile for the whole selection, or one per project when the workspace
 * uses dedicated lockfiles (`sharedWorkspaceLockfile: false`).
 */
function importerIdsByLockfileDir (opts: LicensesCommandOptions): Map<string, ProjectId[]> {
  const projectDirs = opts.selectedProjectsGraph ? Object.keys(opts.selectedProjectsGraph) : [opts.dir]
  const byLockfileDir = new Map<string, ProjectId[]>()
  for (const projectDir of projectDirs) {
    const lockfileDir = opts.lockfileDir ??
      (opts.sharedWorkspaceLockfile === false ? projectDir : opts.dir)
    let importerIds = byLockfileDir.get(lockfileDir)
    if (importerIds == null) {
      importerIds = []
      byLockfileDir.set(lockfileDir, importerIds)
    }
    importerIds.push(getLockfileImporterId(lockfileDir, projectDir))
  }
  return byLockfileDir
}
