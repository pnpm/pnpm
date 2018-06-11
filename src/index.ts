import logger from '@pnpm/logger'
import createResolver from '@pnpm/npm-resolver'
import {fromDir as readPackageFromDir} from '@pnpm/read-package-json'
import resolveStore from '@pnpm/store-path'
import * as dp from 'dependency-path'
import {
  readCurrent as readCurrentShrinkwrap,
  readWanted as readWantedShrinkwrap,
} from 'pnpm-shrinkwrap'

export interface OutdatedPackage {
  packageName: string,
  current?: string, // not defined means the package is not installed
  wanted: string,
  latest?: string,
}

const depTypes = ['dependencies', 'devDependencies', 'optionalDependencies']

export default async function (
  pkgPath: string,
  opts: {
    offline: boolean,
    store: string,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    strictSsl: boolean,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMintimeout: number,
    fetchRetryMaxtimeout: number,
    userAgent: string,
    tag: string,
    networkConcurrency: number,
    rawNpmConfig: object,
    alwaysAuth: boolean,
  },
) {
  return _outdated([], pkgPath, opts)
}

export async function forPackages (
  packages: string[],
  pkgPath: string,
  opts: {
    offline: boolean,
    store: string,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    strictSsl: boolean,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMintimeout: number,
    fetchRetryMaxtimeout: number,
    userAgent: string,
    tag: string,
    networkConcurrency: number,
    rawNpmConfig: object,
    alwaysAuth: boolean,
  },
) {
  return _outdated(packages, pkgPath, opts)
}

async function _outdated (
  forPkgs: string[],
  pkgPath: string,
  opts: {
    offline: boolean,
    store: string,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    strictSsl: boolean,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMintimeout: number,
    fetchRetryMaxtimeout: number,
    userAgent: string,
    tag: string,
    networkConcurrency: number,
    rawNpmConfig: object,
    alwaysAuth: boolean,
  },
): Promise<OutdatedPackage[]> {
  const pkg = await readPackageFromDir(pkgPath)
  if (packageHasNoDeps(pkg)) return []
  const wantedShrinkwrap = await readWantedShrinkwrap(pkgPath, {ignoreIncompatible: false})
    || await readCurrentShrinkwrap(pkgPath, {ignoreIncompatible: false})
  if (!wantedShrinkwrap) {
    throw new Error('No shrinkwrapfile in this directory. Run `pnpm install` to generate one.')
  }
  const storePath = await resolveStore(pkgPath, opts.store)
  const currentShrinkwrap = await readCurrentShrinkwrap(pkgPath, {ignoreIncompatible: false}) || {}

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
    depTypes.map(async (depType) => {
      if (!wantedShrinkwrap[depType]) return

      let pkgs = Object.keys(wantedShrinkwrap[depType])

      if (forPkgs.length) {
        pkgs = pkgs.filter((pkgName) => forPkgs.indexOf(pkgName) !== -1)
      }

      await Promise.all(
        pkgs.map(async (packageName) => {
          const ref = wantedShrinkwrap[depType][packageName]

          // ignoring linked packages
          if (ref.startsWith('file:') || ref.startsWith('link:')) {
            return
          }

          const relativeDepPath = dp.refToRelative(ref, packageName)
          const pkgSnapshot = wantedShrinkwrap.packages && wantedShrinkwrap.packages[relativeDepPath]

          if (!pkgSnapshot) {
            throw new Error(`Invalid shrinkwrap.yaml file. ${relativeDepPath} not found in packages field`)
          }

          // It might be not the best solution to check for pkgSnapshot.name
          // TODO: add some other field to distinct packages not from the registry
          if (pkgSnapshot.resolution && (pkgSnapshot.resolution['type'] || pkgSnapshot.name)) { // tslint:disable-line:no-string-literal
            if (currentShrinkwrap[depType][packageName] !== wantedShrinkwrap[depType][packageName]) {
              outdated.push({
                current: currentShrinkwrap[depType][packageName],
                latest: undefined,
                packageName,
                wanted: wantedShrinkwrap[depType][packageName],
              })
            }
            return
          }

          // TODO: what about aliased dependencies?
          const resolution = await resolve({alias: packageName, pref: 'latest'}, {
            registry: wantedShrinkwrap.registry,
          })

          if (!resolution || !resolution.latest) return

          const latest = resolution.latest

          if (!currentShrinkwrap[depType][packageName]) {
            outdated.push({
              latest,
              packageName,
              wanted: wantedShrinkwrap[depType][packageName],
            })
            return
          }

          if (currentShrinkwrap[depType][packageName] !== wantedShrinkwrap[depType][packageName] ||
            latest !== currentShrinkwrap[depType][packageName]) {
            outdated.push({
              current: currentShrinkwrap[depType][packageName],
              latest,
              packageName,
              wanted: wantedShrinkwrap[depType][packageName],
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

function adaptConfig (
  opts: {
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    strictSsl: boolean,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMintimeout: number,
    fetchRetryMaxtimeout: number,
    userAgent: string,
    tag: string,
  },
) {
  const registryLog = logger('registry')
  return {
    defaultTag: opts.tag,
    log: Object.assign({}, registryLog, {
      http: registryLog.debug.bind(null, 'http'),
      verbose: registryLog.debug.bind(null, 'http'),
    }),
    proxy: {
      http: opts.proxy,
      https: opts.httpsProxy,
      localAddress: opts.localAddress,
    },
    retry: {
      count: opts.fetchRetries,
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
    },
    ssl: {
      ca: opts.ca,
      certificate: opts.cert,
      key: opts.key,
      strict: opts.strictSsl,
    },
    userAgent: opts.userAgent,
  }
}
