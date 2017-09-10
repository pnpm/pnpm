import RegClient = require('npm-registry-client')
import {
  read as readWantedShrinkwrap,
  readPrivate as readCurrentShrinkwrap
} from 'pnpm-shrinkwrap'
import {
  resolve,
  createGot,
  PackageMeta,
} from 'package-store'
import npa = require('npm-package-arg')
import logger from 'pnpm-logger'

export type OutdatedPackage = {
  packageName: string,
  current?: string, // not defined means the package is not installed
  wanted: string,
  latest: string,
}

const depTypes = ['dependencies', 'devDependencies', 'optionalDependencies']

export default async function (
  pkgPath: string,
  opts: {
    offline: boolean,
    storePath: string,
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
    rawNpmConfig: Object,
    alwaysAuth: boolean,
  }
) {
  return _outdated([], pkgPath, opts)
}

export async function forPackages (
  packages: string[],
  pkgPath: string,
  opts: {
    offline: boolean,
    storePath: string,
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
    rawNpmConfig: Object,
    alwaysAuth: boolean,
  }
) {
  return _outdated(packages, pkgPath, opts)
}

async function _outdated (
  forPkgs: string[],
  pkgPath: string,
  opts: {
    offline: boolean,
    storePath: string,
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
    rawNpmConfig: Object,
    alwaysAuth: boolean,
  }
): Promise<OutdatedPackage[]> {
  const wantedShrinkwrap = await readWantedShrinkwrap(pkgPath, {ignoreIncompatible: false})
  if (!wantedShrinkwrap) {
    throw new Error('No shrinkwrapfile in this directory. Run `pnpm install` to generate one.')
  }
  const currentShrinkwrap = await readCurrentShrinkwrap(pkgPath, {ignoreIncompatible: false}) || {}

  const client = new RegClient(adaptConfig(opts))
  const got = createGot(client, {
    networkConcurrency: opts.networkConcurrency,
    rawNpmConfig: opts.rawNpmConfig,
    alwaysAuth: opts.alwaysAuth,
    registry: wantedShrinkwrap.registry,
    retries: opts.fetchRetries,
    factor: opts.fetchRetryFactor,
    maxTimeout: opts.fetchRetryMaxtimeout,
    minTimeout: opts.fetchRetryMintimeout,
  })
  const metaCache = new Map<string, PackageMeta>()

  const outdated: OutdatedPackage[] = []

  await Promise.all(
    depTypes.map(async depType => {
      if (!wantedShrinkwrap[depType]) return

      let pkgs = Object.keys(wantedShrinkwrap[depType])

      if (forPkgs.length) {
        pkgs = pkgs.filter(pkgName => forPkgs.indexOf(pkgName) !== -1)
      }

      await Promise.all(
        pkgs.map(async packageName => {
          const resolution = await resolve(npa.resolve(packageName, 'latest'), {
            downloadPriority: 0,
            got,
            registry: wantedShrinkwrap.registry,
            metaCache,
            offline: opts.offline,
            prefix: pkgPath,
            loggedPkg: {
              rawSpec: `${packageName}@latest`,
              name: packageName,
            },
            storePath: opts.storePath,
          })

          if (!resolution || !resolution.package) return

          const latest = resolution.package.version

          if (!currentShrinkwrap[depType][packageName]) {
            outdated.push({
              packageName,
              wanted: wantedShrinkwrap[depType][packageName],
              latest,
            })
            return
          }

          if (currentShrinkwrap[depType][packageName] !== wantedShrinkwrap[depType][packageName] ||
            latest !== currentShrinkwrap[depType][packageName]) {
            outdated.push({
              packageName,
              current: currentShrinkwrap[depType][packageName],
              wanted: wantedShrinkwrap[depType][packageName],
              latest,
            })
          }
        })
      )
    })
  )

  return outdated.sort((pkg1, pkg2) => pkg1.packageName.localeCompare(pkg2.packageName))
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
  }
) {
  const registryLog = logger('registry')
  return {
    proxy: {
      http: opts.proxy,
      https: opts.httpsProxy,
      localAddress: opts.localAddress
    },
    ssl: {
      certificate: opts.cert,
      key: opts.key,
      ca: opts.ca,
      strict: opts.strictSsl
    },
    retry: {
      count: opts.fetchRetries,
      factor: opts.fetchRetryFactor,
      minTimeout: opts.fetchRetryMintimeout,
      maxTimeout: opts.fetchRetryMaxtimeout
    },
    userAgent: opts.userAgent,
    log: Object.assign({}, registryLog, {
      verbose: registryLog.debug.bind(null, 'http'),
      http: registryLog.debug.bind(null, 'http'),
    }),
    defaultTag: opts.tag
  }
}
