import { Config } from '@pnpm/config'
import { createFetchFromRegistry } from '@pnpm/fetch'
import createResolver from '@pnpm/npm-resolver'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import { ResolveFunction } from '@pnpm/resolver-base'
import runNpm from '@pnpm/run-npm'
import sortPackages from '@pnpm/sort-packages'
import storePath from '@pnpm/store-path'
import { Registries } from '@pnpm/types'
import getCredentialsByURI = require('credentials-by-uri')
import LRU = require('lru-cache')
import mem = require('mem')
import pFilter = require('p-filter')
import { handler as publish } from './publish'

export type PublishRecursiveOpts = Required<Pick<Config,
  | 'cliOptions'
  | 'dir'
  | 'rawConfig'
  | 'registries'
  | 'workspaceDir'
>> &
Partial<Pick<Config,
  | 'tag'
  | 'ca'
  | 'cert'
  | 'extraBinPaths'
  | 'fetchRetries'
  | 'fetchRetryFactor'
  | 'fetchRetryMaxtimeout'
  | 'fetchRetryMintimeout'
  | 'key'
  | 'localAddress'
  | 'lockfileDir'
  | 'npmPath'
  | 'offline'
  | 'proxy'
  | 'selectedProjectsGraph'
  | 'storeDir'
  | 'strictSsl'
  | 'userAgent'
  | 'verifyStoreIntegrity'
>> & {
  access?: 'public' | 'restricted',
  argv: {
    original: string[],
  },
}

export default async function (
  opts: PublishRecursiveOpts & Required<Pick<Config, 'selectedProjectsGraph'>>
) {
  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
  const storeDir = await storePath(opts.workspaceDir, opts.storeDir)
  const fetch = createFetchFromRegistry(opts)
  const getCredentials = mem((registry: string) => getCredentialsByURI(opts.rawConfig, registry))
  const resolve = createResolver(fetch, getCredentials, Object.assign(opts, {
    metaCache: new LRU({
      max: 10000,
      maxAge: 120 * 1000, // 2 minutes
    }) as any, // tslint:disable-line:no-any
    storeDir,
  })) as unknown as ResolveFunction
  const pkgsToPublish = await pFilter(pkgs, async (pkg) => {
    if (!pkg.manifest.name || !pkg.manifest.version || pkg.manifest.private) return false
    return !(await isAlreadyPublished({
      dir: pkg.dir,
      lockfileDir: opts.lockfileDir || pkg.dir,
      registries: opts.registries,
      resolve,
    }, pkg.manifest.name, pkg.manifest.version))
  })
  const publishedPkgDirs = new Set(pkgsToPublish.map(({ dir }) => dir))
  const access = opts.cliOptions['access'] ? ['--access', opts.cliOptions['access']] : []
  const chunks = sortPackages(opts.selectedProjectsGraph)
  for (const chunk of chunks) {
    for (const pkgDir of chunk) {
      if (!publishedPkgDirs.has(pkgDir)) continue
      const pkg = opts.selectedProjectsGraph[pkgDir].package
      await publish({
        ...opts,
        argv: {
          original: [
            'publish',
            pkg.dir,
            '--tag',
            'pnpm-temp',
            '--registry',
            pickRegistryForPackage(opts.registries, pkg.manifest.name!),
            ...access,
          ],
        },
        gitChecks: false,
        recursive: false,
      }, [pkg.dir])
    }
  }
  const tag = opts.tag || 'latest'
  for (const pkg of pkgsToPublish) {
    runNpm(opts.npmPath, [
      'dist-tag',
      'add',
      `${pkg.manifest.name}@${pkg.manifest.version}`,
      tag,
      '--registry',
      pickRegistryForPackage(opts.registries, pkg.manifest.name!),
    ])
  }
}

async function isAlreadyPublished (
  opts: {
    dir: string,
    lockfileDir: string,
    registries: Registries,
    resolve: ResolveFunction,
  },
  pkgName: string,
  pkgVersion: string
) {
  try {
    await opts.resolve({ alias: pkgName, pref: pkgVersion }, {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.dir,
      registry: pickRegistryForPackage(opts.registries, pkgName),
    })
    return true
  } catch (err) {
    return false
  }
}
