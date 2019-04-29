import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  getLockfileImporterId,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import createResolver from '@pnpm/npm-resolver'
import { readImporterManifestOnly } from '@pnpm/read-importer-manifest'
import resolveStore from '@pnpm/store-path'
import { DEPENDENCIES_FIELDS, ImporterManifest, Registries } from '@pnpm/types'
import { normalizeRegistries } from '@pnpm/utils'
import * as dp from 'dependency-path'

export interface OutdatedPackage {
  current?: string, // not defined means the package is not installed
  latest?: string,
  packageName: string,
  wanted: string,
}

export default async function (
  pkgPath: string,
  opts: {
    alwaysAuth: boolean,
    ca?: string,
    cert?: string,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMaxtimeout: number,
    fetchRetryMintimeout: number,
    httpsProxy?: string,
    key?: string,
    localAddress?: string,
    networkConcurrency: number,
    offline: boolean,
    proxy?: string,
    rawNpmConfig: object,
    registries?: Registries,
    lockfileDirectory?: string,
    store: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
) {
  return _outdated([], pkgPath, opts)
}

export async function forPackages (
  packages: string[],
  pkgPath: string,
  opts: {
    alwaysAuth: boolean,
    ca?: string,
    cert?: string,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMaxtimeout: number,
    fetchRetryMintimeout: number,
    httpsProxy?: string,
    key?: string,
    localAddress?: string,
    networkConcurrency: number,
    offline: boolean,
    proxy?: string,
    rawNpmConfig: object,
    registries?: Registries,
    lockfileDirectory?: string,
    store: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
) {
  return _outdated(packages, pkgPath, opts)
}

async function _outdated (
  forPkgs: string[],
  pkgPath: string,
  opts: {
    alwaysAuth: boolean,
    ca?: string,
    cert?: string,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMaxtimeout: number,
    fetchRetryMintimeout: number,
    httpsProxy?: string,
    key?: string,
    localAddress?: string,
    networkConcurrency: number,
    offline: boolean,
    proxy?: string,
    rawNpmConfig: object,
    registries?: Registries,
    lockfileDirectory?: string,
    store: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
): Promise<OutdatedPackage[]> {
  const registries = normalizeRegistries(opts.registries)
  const lockfileDirectory = opts.lockfileDirectory || pkgPath
  const manifest = await readImporterManifestOnly(pkgPath)
  if (packageHasNoDeps(manifest)) return []
  const wantedLockfile = await readWantedLockfile(lockfileDirectory, { ignoreIncompatible: false })
    || await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false })
  if (!wantedLockfile) {
    throw new Error('No lockfile in this directory. Run `pnpm install` to generate one.')
  }
  const storePath = await resolveStore(pkgPath, opts.store)
  const importerId = getLockfileImporterId(lockfileDirectory, pkgPath)
  const currentLockfile = await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false }) || { importers: { [importerId]: {} } }

  const resolve = createResolver({
    fetchRetries: opts.fetchRetries,
    fetchRetryFactor: opts.fetchRetryFactor,
    fetchRetryMaxtimeout: opts.fetchRetryMaxtimeout,
    fetchRetryMintimeout: opts.fetchRetryMintimeout,
    metaCache: new Map<string, object>() as any, // tslint:disable-line
    offline: opts.offline,
    rawNpmConfig: opts.rawNpmConfig,
    store: storePath,
  })

  const outdated: OutdatedPackage[] = []

  await Promise.all(
    DEPENDENCIES_FIELDS.map(async (depType) => {
      if (!wantedLockfile.importers[importerId][depType]) return

      let pkgs = Object.keys(wantedLockfile.importers[importerId][depType]!)

      if (forPkgs.length) {
        pkgs = pkgs.filter((pkgName) => forPkgs.includes(pkgName))
      }

      await Promise.all(
        pkgs.map(async (packageName) => {
          const ref = wantedLockfile.importers[importerId][depType]![packageName]

          // ignoring linked packages. (For backward compatibility)
          if (ref.startsWith('file:')) {
            return
          }

          const relativeDepPath = dp.refToRelative(ref, packageName)

          // ignoring linked packages
          if (relativeDepPath === null) return

          const pkgSnapshot = wantedLockfile.packages && wantedLockfile.packages[relativeDepPath]

          if (!pkgSnapshot) {
            throw new Error(`Invalid ${WANTED_LOCKFILE} file. ${relativeDepPath} not found in packages field`)
          }

          const currentRef = currentLockfile.importers[importerId][depType][packageName]
          const currentRelative = currentRef && dp.refToRelative(currentRef, packageName)
          const current = currentRelative && dp.parse(currentRelative).version || currentRef
          const wanted = dp.parse(relativeDepPath).version || ref

          // It might be not the best solution to check for pkgSnapshot.name
          // TODO: add some other field to distinct packages not from the registry
          if (pkgSnapshot.resolution && (pkgSnapshot.resolution['type'] || pkgSnapshot.name)) { // tslint:disable-line:no-string-literal
            if (current !== wanted) {
              outdated.push({
                current,
                latest: undefined,
                packageName,
                wanted,
              })
            }
            return
          }

          // TODO: what about aliased dependencies?
          // TODO: what about scoped dependencies?
          const resolution = await resolve({ alias: packageName, pref: 'latest' }, {
            registry: registries.default,
          })

          if (!resolution || !resolution.latest) return

          const latest = resolution.latest

          if (!current) {
            outdated.push({
              latest,
              packageName,
              wanted,
            })
            return
          }

          if (current !== wanted || latest !== current) {
            outdated.push({
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
