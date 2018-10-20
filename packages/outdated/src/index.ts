import createResolver from '@pnpm/npm-resolver'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import resolveStore from '@pnpm/store-path'
import { DEPENDENCIES_FIELDS, Registries } from '@pnpm/types'
import { normalizeRegistries } from '@pnpm/utils'
import * as dp from 'dependency-path'
import {
  getImporterId,
  readCurrent as readCurrentShrinkwrap,
  readWanted as readWantedShrinkwrap,
} from 'pnpm-shrinkwrap'

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
    shrinkwrapDirectory?: string,
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
    shrinkwrapDirectory?: string,
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
    shrinkwrapDirectory?: string,
    store: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
): Promise<OutdatedPackage[]> {
  const registries = normalizeRegistries(opts.registries)
  const shrinkwrapDirectory = opts.shrinkwrapDirectory || pkgPath
  const pkg = await readPackageFromDir(pkgPath)
  if (packageHasNoDeps(pkg)) return []
  const wantedShrinkwrap = await readWantedShrinkwrap(shrinkwrapDirectory, { ignoreIncompatible: false })
    || await readCurrentShrinkwrap(shrinkwrapDirectory, { ignoreIncompatible: false })
  if (!wantedShrinkwrap) {
    throw new Error('No shrinkwrapfile in this directory. Run `pnpm install` to generate one.')
  }
  const storePath = await resolveStore(pkgPath, opts.store)
  const importerId = getImporterId(shrinkwrapDirectory, pkgPath)
  const currentShrinkwrap = await readCurrentShrinkwrap(shrinkwrapDirectory, { ignoreIncompatible: false }) || { importers: { [importerId]: {} } }

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
      if (!wantedShrinkwrap.importers[importerId][depType]) return

      let pkgs = Object.keys(wantedShrinkwrap.importers[importerId][depType]!)

      if (forPkgs.length) {
        pkgs = pkgs.filter((pkgName) => forPkgs.indexOf(pkgName) !== -1)
      }

      await Promise.all(
        pkgs.map(async (packageName) => {
          const ref = wantedShrinkwrap.importers[importerId][depType]![packageName]

          // ignoring linked packages. (For backward compatibility)
          if (ref.startsWith('file:')) {
            return
          }

          const relativeDepPath = dp.refToRelative(ref, packageName)

          // ignoring linked packages
          if (relativeDepPath === null) return

          const pkgSnapshot = wantedShrinkwrap.packages && wantedShrinkwrap.packages[relativeDepPath]

          if (!pkgSnapshot) {
            throw new Error(`Invalid shrinkwrap.yaml file. ${relativeDepPath} not found in packages field`)
          }

          // It might be not the best solution to check for pkgSnapshot.name
          // TODO: add some other field to distinct packages not from the registry
          if (pkgSnapshot.resolution && (pkgSnapshot.resolution['type'] || pkgSnapshot.name)) { // tslint:disable-line:no-string-literal
            if (currentShrinkwrap.importers[importerId][depType][packageName] !== wantedShrinkwrap.importers[importerId][depType]![packageName]) {
              outdated.push({
                current: currentShrinkwrap.importers[importerId][depType]![packageName],
                latest: undefined,
                packageName,
                wanted: wantedShrinkwrap.importers[importerId][depType]![packageName],
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

          if (!currentShrinkwrap.importers[importerId][depType][packageName]) {
            outdated.push({
              latest,
              packageName,
              wanted: wantedShrinkwrap.importers[importerId][depType]![packageName],
            })
            return
          }

          if (currentShrinkwrap.importers[importerId][depType][packageName] !== wantedShrinkwrap.importers[importerId][depType]![packageName] ||
            latest !== currentShrinkwrap.importers[importerId][depType][packageName]) {
            outdated.push({
              current: currentShrinkwrap.importers[importerId][depType][packageName],
              latest,
              packageName,
              wanted: wantedShrinkwrap.importers[importerId][depType]![packageName],
            })
          }
        }),
      )
    }),
  )

  return outdated.sort((pkg1, pkg2) => pkg1.packageName.localeCompare(pkg2.packageName))
}

// tslint:disable-next-line:no-any
function packageHasNoDeps (pkg: any) {
  return (!pkg.dependencies || isEmpty(pkg.dependencies)
    && (!pkg.devDependencies || isEmpty(pkg.devDependencies))
    && (!pkg.optionalDependencies || isEmpty(pkg.optionalDependencies)))
}

function isEmpty (obj: object) {
  return Object.keys(obj).length === 0
}
