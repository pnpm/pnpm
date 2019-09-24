import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  getLockfileImporterId,
  Lockfile,
} from '@pnpm/lockfile-file'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  ImporterManifest,
  PackageManifest,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import { isMatch } from 'micromatch'

export type GetLatestManifestFunction = (packageName: string) => Promise<PackageManifest | null>

export interface OutdatedPackage {
  alias: string,
  belongsTo: DependenciesField,
  current?: string, // not defined means the package is not installed
  latestManifest?: PackageManifest,
  packageName: string,
  wanted: string,
}

export default async function (
  opts: {
    currentLockfile: Lockfile | null,
    manifest: ImporterManifest,
    prefix: string,
    getLatestManifest: GetLatestManifestFunction,
    lockfileDirectory: string,
    wantedLockfile: Lockfile,
  },
) {
  return _outdated([], opts)
}

export async function forPackages (
  packages: string[],
  opts: {
    currentLockfile: Lockfile | null,
    manifest: ImporterManifest,
    prefix: string,
    getLatestManifest: GetLatestManifestFunction,
    lockfileDirectory: string,
    wantedLockfile: Lockfile,
  },
) {
  return _outdated(packages, opts)
}

async function _outdated (
  forPkgs: string[],
  opts: {
    manifest: ImporterManifest,
    prefix: string,
    currentLockfile: Lockfile | null,
    getLatestManifest: GetLatestManifestFunction,
    lockfileDirectory: string,
    wantedLockfile: Lockfile,
  },
): Promise<OutdatedPackage[]> {
  if (packageHasNoDeps(opts.manifest)) return []
  const importerId = getLockfileImporterId(opts.lockfileDirectory, opts.prefix)
  const currentLockfile = opts.currentLockfile || { importers: { [importerId]: {} } }

  const outdated: OutdatedPackage[] = []

  await Promise.all(
    DEPENDENCIES_FIELDS.map(async (depType) => {
      if (!opts.wantedLockfile.importers[importerId][depType]) return

      let pkgs = Object.keys(opts.wantedLockfile.importers[importerId][depType]!)

      if (forPkgs.length) {
        pkgs = pkgs.filter((pkgName) => forPkgs.some((forPkg) => isMatch(pkgName, forPkg)))
      }

      await Promise.all(
        pkgs.map(async (alias) => {
          const ref = opts.wantedLockfile.importers[importerId][depType]![alias]

          // ignoring linked packages. (For backward compatibility)
          if (ref.startsWith('file:')) {
            return
          }

          const relativeDepPath = dp.refToRelative(ref, alias)

          // ignoring linked packages
          if (relativeDepPath === null) return

          const pkgSnapshot = opts.wantedLockfile.packages && opts.wantedLockfile.packages[relativeDepPath]

          if (!pkgSnapshot) {
            throw new Error(`Invalid ${WANTED_LOCKFILE} file. ${relativeDepPath} not found in packages field`)
          }

          const currentRef = currentLockfile.importers[importerId] &&
            currentLockfile.importers[importerId][depType] &&
            currentLockfile.importers[importerId][depType]![alias]
          const currentRelative = currentRef && dp.refToRelative(currentRef, alias)
          const current = currentRelative && dp.parse(currentRelative).version || currentRef
          const wanted = dp.parse(relativeDepPath).version || ref
          const packageName = nameVerFromPkgSnapshot(relativeDepPath, pkgSnapshot).name

          // It might be not the best solution to check for pkgSnapshot.name
          // TODO: add some other field to distinct packages not from the registry
          if (pkgSnapshot.resolution && (pkgSnapshot.resolution['type'] || pkgSnapshot.name)) { // tslint:disable-line:no-string-literal
            if (current !== wanted) {
              outdated.push({
                alias,
                belongsTo: depType,
                current,
                latestManifest: undefined,
                packageName,
                wanted,
              })
            }
            return
          }

          const latestManifest = await opts.getLatestManifest(dp.parse(relativeDepPath).name || packageName)

          if (!latestManifest) return

          if (!current) {
            outdated.push({
              alias,
              belongsTo: depType,
              latestManifest,
              packageName,
              wanted,
            })
            return
          }

          if (current !== wanted || latestManifest.version !== current || latestManifest.deprecated) {
            outdated.push({
              alias,
              belongsTo: depType,
              current,
              latestManifest,
              packageName,
              wanted,
            })
          }
        }),
      )
    }),
  )

  return outdated.sort((pkg1, pkg2) => pkg1.packageName.localeCompare(pkg2.packageName))
}

function packageHasNoDeps (manifest: ImporterManifest) {
  return (!manifest.dependencies || isEmpty(manifest.dependencies))
    && (!manifest.devDependencies || isEmpty(manifest.devDependencies))
    && (!manifest.optionalDependencies || isEmpty(manifest.optionalDependencies))
}

function isEmpty (obj: object) {
  return Object.keys(obj).length === 0
}
