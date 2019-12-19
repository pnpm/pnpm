import createResolver from '@pnpm/npm-resolver'
import { publish } from '@pnpm/plugin-commands-publishing'
import { ResolveFunction } from '@pnpm/resolver-base'
import runNpm from '@pnpm/run-npm'
import storePath from '@pnpm/store-path'
import { ImporterManifest, Registries } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/utils'
import LRU = require('lru-cache')
import pFilter = require('p-filter')

export default async function (
  pkgs: Array<{ dir: string, manifest: ImporterManifest }>,
  opts: {
    access?: 'public' | 'restricted',
    argv: {
      original: string[],
    },
    tag?: string,
    ca?: string,
    cert?: string,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMaxtimeout?: number,
    fetchRetryMintimeout?: number,
    httpsProxy?: string,
    key?: string,
    localAddress?: string,
    lockfileDir?: string,
    offline?: boolean,
    dir: string,
    proxy?: string,
    rawConfig: object,
    registries: Registries,
    storeDir?: string,
    strictSsl?: boolean,
    userAgent?: string,
    verifyStoreIntegrity?: boolean,
    workspaceDir: string,
  },
) {
  const storeDir = await storePath(opts.workspaceDir, opts.storeDir)
  const resolve = createResolver(Object.assign(opts, {
    fullMetadata: true,
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
  for (const pkg of pkgsToPublish) {
    const access = opts.access ?? (pkg.manifest.name!.startsWith('@') ? 'restricted' : 'public')
    await publish.handler([pkg.dir], {
      argv: {
        original: [
          'publish',
          pkg.dir,
          '--tag',
          'pnpm-temp',
          '--registry',
          pickRegistryForPackage(opts.registries, pkg.manifest.name!),
          '--access',
          access,
        ],
      },
      workspaceDir: opts.workspaceDir,
    }, 'publish')
  }
  const tag = opts.tag || 'latest'
  for (const pkg of pkgsToPublish) {
    await runNpm([
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
  pkgVersion: string,
) {
  try {
    await opts.resolve({ alias: pkgName, pref: pkgVersion }, {
      importerDir: opts.dir,
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      registry: pickRegistryForPackage(opts.registries, pkgName),
    })
    return true
  } catch (err) {
    return false
  }
}
