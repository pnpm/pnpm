import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  getLockfileImporterId,
  Lockfile,
} from '@pnpm/lockfile-file'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { DEPENDENCIES_FIELDS, ImporterManifest } from '@pnpm/types'
import * as dp from 'dependency-path'

export type GetLatestVersionFunction = (packageName: string) => Promise<string | null>

export interface OutdatedPackage {
  alias: string,
  current?: string, // not defined means the package is not installed
  latest?: string,
  packageName: string,
  wanted: string,
}

export default async function (
  opts: {
    currentLockfile: Lockfile | null,
    manifest: ImporterManifest,
    prefix: string,
    getLatestVersion: GetLatestVersionFunction,
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
    getLatestVersion: GetLatestVersionFunction,
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
    getLatestVersion: GetLatestVersionFunction,
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
        pkgs = pkgs.filter((pkgName) => forPkgs.includes(pkgName))
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
                current,
                latest: undefined,
                packageName,
                wanted,
              })
            }
            return
          }

          const latest = await opts.getLatestVersion(dp.parse(relativeDepPath).name || packageName)

          if (!latest) return

          if (!current) {
            outdated.push({
              alias,
              latest,
              packageName,
              wanted,
            })
            return
          }

          if (current !== wanted || latest !== current) {
            outdated.push({
              alias,
              current,
              latest,
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
