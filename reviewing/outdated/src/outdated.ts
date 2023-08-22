import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import {
  getLockfileImporterId,
  type Lockfile,
  type ProjectSnapshot,
} from '@pnpm/lockfile-file'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { parsePref } from '@pnpm/npm-resolver'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import {
  type DependenciesField,
  DEPENDENCIES_FIELDS,
  type IncludedDependencies,
  type PackageManifest,
  type ProjectManifest,
  type Registries,
} from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import semver from 'semver'
import { createMatcher } from '@pnpm/matcher'

export * from './createManifestGetter'

export type GetLatestManifestFunction = (packageName: string, rangeOrTag: string) => Promise<PackageManifest | null>

export interface OutdatedPackage {
  alias: string
  belongsTo: DependenciesField
  current?: string // not defined means the package is not installed
  latestManifest?: PackageManifest
  packageName: string
  wanted: string
  workspace?: string
}

export async function outdated (
  opts: {
    compatible?: boolean
    currentLockfile: Lockfile | null
    getLatestManifest: GetLatestManifestFunction
    ignoreDependencies?: string[]
    include?: IncludedDependencies
    lockfileDir: string
    manifest: ProjectManifest
    match?: (dependencyName: string) => boolean
    prefix: string
    registries: Registries
    wantedLockfile: Lockfile | null
  }
): Promise<OutdatedPackage[]> {
  if (packageHasNoDeps(opts.manifest)) return []
  if (opts.wantedLockfile == null) {
    throw new PnpmError('OUTDATED_NO_LOCKFILE', `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`)
  }
  const allDeps = getAllDependenciesFromManifest(opts.manifest)
  const importerId = getLockfileImporterId(opts.lockfileDir, opts.prefix)
  const currentLockfile = opts.currentLockfile ?? { importers: { [importerId]: {} } }

  const outdated: OutdatedPackage[] = []

  const ignoreDependenciesMatcher = opts.ignoreDependencies?.length ? createMatcher(opts.ignoreDependencies) : undefined

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
            ignoreDependenciesMatcher?.(alias)
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

          const currentRef = (currentLockfile.importers[importerId] as ProjectSnapshot)?.[depType]?.[alias]
          const currentRelative = currentRef && dp.refToRelative(currentRef, alias)
          const current = (currentRelative && dp.parse(currentRelative).version) ?? currentRef
          const wanted = dp.parse(relativeDepPath).version ?? ref
          const { name: packageName } = nameVerFromPkgSnapshot(relativeDepPath, pkgSnapshot)
          const name = dp.parse(relativeDepPath).name ?? packageName

          // If the npm resolve parser cannot parse the spec of the dependency,
          // it means that the package is not from a npm-compatible registry.
          // In that case, we can't check whether the package is up-to-date
          if (
            parsePref(allDeps[alias], alias, 'latest', pickRegistryForPackage(opts.registries, name)) == null
          ) {
            if (current !== wanted) {
              outdated.push({
                alias,
                belongsTo: depType,
                current,
                latestManifest: undefined,
                packageName,
                wanted,
                workspace: opts.manifest.name,
              })
            }
            return
          }

          const latestManifest = await opts.getLatestManifest(
            name,
            opts.compatible ? (allDeps[name] ?? 'latest') : 'latest'
          )

          if (latestManifest == null) return

          if (!current) {
            outdated.push({
              alias,
              belongsTo: depType,
              latestManifest,
              packageName,
              wanted,
              workspace: opts.manifest.name,

            })
            return
          }

          if (current !== wanted || semver.lt(current, latestManifest.version) || latestManifest.deprecated) {
            outdated.push({
              alias,
              belongsTo: depType,
              current,
              latestManifest,
              packageName,
              wanted,
              workspace: opts.manifest.name,

            })
          }
        })
      )
    })
  )

  return outdated.sort((pkg1, pkg2) => pkg1.packageName.localeCompare(pkg2.packageName))
}

function packageHasNoDeps (manifest: ProjectManifest) {
  return ((manifest.dependencies == null) || isEmpty(manifest.dependencies)) &&
    ((manifest.devDependencies == null) || isEmpty(manifest.devDependencies)) &&
    ((manifest.optionalDependencies == null) || isEmpty(manifest.optionalDependencies))
}

function isEmpty (obj: object) {
  return Object.keys(obj).length === 0
}
