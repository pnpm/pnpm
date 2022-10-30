import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import {
  getLockfileImporterId,
  Lockfile,
} from '@pnpm/lockfile-file'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import {
  DependenciesField,
  DEPENDENCIES_FIELDS,
  IncludedDependencies,
  PackageManifest,
  ProjectManifest,
  Registries,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import { getPkgInfo } from './getPkgInfo'

export interface LicensePackage {
  alias: string
  belongsTo: DependenciesField
  version: string
  packageManifest?: PackageManifest
  packageName: string
  license: string
  licenseContents?: string
  author?: string
  packageDirectory?: string
}

export async function licences (
  opts: {
    compatible?: boolean
    currentLockfile: Lockfile | null
    ignoreDependencies?: Set<string>
    include?: IncludedDependencies
    lockfileDir: string
    manifest: ProjectManifest
    match?: (dependencyName: string) => boolean
    prefix: string
    registries: Registries
    wantedLockfile: Lockfile | null
  }
): Promise<LicensePackage[]> {
  if (packageHasNoDeps(opts.manifest)) return []
  if (opts.wantedLockfile == null) {
    throw new PnpmError('LICENSES_NO_LOCKFILE', `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`)
  }
  const allDeps = getAllDependenciesFromManifest(opts.manifest)
  const importerId = getLockfileImporterId(opts.lockfileDir, opts.prefix)
  const licenses: LicensePackage[] = []

  await Promise.all(
    DEPENDENCIES_FIELDS.map(async (depType) => {
      if (
        opts.include?.[depType] === false ||
        (opts.wantedLockfile!.importers[importerId][depType] == null)
      ) return

      let pkgs = Object.keys(opts.wantedLockfile!.importers[importerId][depType]!)
      if (opts.match != null) {
        pkgs = pkgs.filter((pkgName) => opts.match!(pkgName))
      }

      await Promise.all(
        pkgs.map(async (alias) => {
          if (!allDeps[alias]) return
          const ref = opts.wantedLockfile!.importers[importerId][depType]![alias]
          if (
            ref.startsWith('file:') || // ignoring linked packages. (For backward compatibility)
            opts.ignoreDependencies?.has(alias)
          ) {
            return
          }

          const relativeDepPath = dp.refToRelative(ref, alias)

          // ignoring linked packages
          if (relativeDepPath === null) return

          const pkgSnapshot = opts.wantedLockfile!.packages?.[relativeDepPath]
          if (pkgSnapshot == null) {
            throw new Error(`Invalid ${WANTED_LOCKFILE} file. ${relativeDepPath} not found in packages field`)
          }

          const { name: packageName, version: packageVersion } = nameVerFromPkgSnapshot(relativeDepPath, pkgSnapshot)
          const name = dp.parse(relativeDepPath).name ?? packageName

          // Fetch the most recent package by the give name
          const { packageManifest, packageInfo } = await getPkgInfo({
            alias,
            name,
            version: packageVersion,
            prefix: opts.prefix,
          })

          licenses.push({
            alias,
            belongsTo: depType,
            version: packageVersion,
            packageManifest,
            packageName,
            license: packageInfo.license,
            licenseContents: packageInfo.licenseContents,
            author: packageInfo.author,
            packageDirectory: packageInfo.path,
          })
        })
      )
    })
  )

  return licenses.sort((pkg1, pkg2) => pkg1.packageName.localeCompare(pkg2.packageName))
}

function packageHasNoDeps (manifest: ProjectManifest) {
  return ((manifest.dependencies == null) || isEmpty(manifest.dependencies)) &&
    ((manifest.devDependencies == null) || isEmpty(manifest.devDependencies)) &&
    ((manifest.optionalDependencies == null) || isEmpty(manifest.optionalDependencies))
}

function isEmpty (obj: object) {
  return Object.keys(obj).length === 0
}
