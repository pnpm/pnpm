import { createResolver } from '@pnpm/client'
import { Config } from '@pnpm/config'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import { ResolveFunction } from '@pnpm/resolver-base'
import sortPackages from '@pnpm/sort-packages'
import storePath from '@pnpm/store-path'
import { Registries } from '@pnpm/types'
import { handler as publish } from './publish'
import pFilter = require('p-filter')

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
| 'dryRun'
| 'extraBinPaths'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'key'
| 'httpProxy'
| 'httpsProxy'
| 'localAddress'
| 'lockfileDir'
| 'noProxy'
| 'npmPath'
| 'offline'
| 'selectedProjectsGraph'
| 'storeDir'
| 'strictSsl'
| 'userAgent'
| 'verifyStoreIntegrity'
>> & {
  access?: 'public' | 'restricted'
  argv: {
    original: string[]
  }
}

export default async function (
  opts: PublishRecursiveOpts & Required<Pick<Config, 'selectedProjectsGraph'>>
) {
  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
  const storeDir = await storePath(opts.workspaceDir, opts.storeDir)
  const resolve = createResolver(Object.assign(opts, {
    authConfig: opts.rawConfig,
    storeDir,
  })) as unknown as ResolveFunction
  const pkgsToPublish = await pFilter(pkgs, async (pkg) => {
    if (!pkg.manifest.name || !pkg.manifest.version || pkg.manifest.private) return false
    return !(await isAlreadyPublished({
      dir: pkg.dir,
      lockfileDir: opts.lockfileDir ?? pkg.dir,
      registries: opts.registries,
      resolve,
    }, pkg.manifest.name, pkg.manifest.version))
  })
  const publishedPkgDirs = new Set(pkgsToPublish.map(({ dir }) => dir))
  const appendedArgs = []
  if (opts.cliOptions['access']) {
    appendedArgs.push(`--access=${opts.cliOptions['access'] as string}`)
  }
  if (opts.dryRun) {
    appendedArgs.push('--dry-run')
  }
  const chunks = sortPackages(opts.selectedProjectsGraph)
  const tag = opts.tag ?? 'latest'
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
            tag,
            '--registry',
            pickRegistryForPackage(opts.registries, pkg.manifest.name!),
            ...appendedArgs,
          ],
        },
        gitChecks: false,
        recursive: false,
      }, [pkg.dir])
    }
  }
}

async function isAlreadyPublished (
  opts: {
    dir: string
    lockfileDir: string
    registries: Registries
    resolve: ResolveFunction
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
