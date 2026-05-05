import path from 'node:path'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { createResolver } from '@pnpm/installing.client'
import { logger } from '@pnpm/logger'
import type { ResolveFunction } from '@pnpm/resolving.resolver-base'
import type { ProjectRootDir, Registries } from '@pnpm/types'
import { sortProjects } from '@pnpm/workspace.projects-sorter'
import pFilter from 'p-filter'
import { pick } from 'ramda'
import { writeJsonFile } from 'write-json-file'

import { publish } from './publish.js'
import type { PublishPackedPkgOptions, PublishSummary } from './publishPackedPkg.js'

export type PublishRecursiveOpts = Required<Pick<Config,
| 'bin'
| 'cacheDir'
| 'dir'
| 'pnpmHomeDir'
| 'configByUri'
| 'registries'
| 'workspaceDir'
>> &
Required<Pick<ConfigContext,
| 'cliOptions'
>> &
Partial<Pick<Config,
| 'tag'
| 'ca'
| 'catalogs'
| 'cert'
| 'fetchTimeout'
| 'force'
| 'dryRun'
| 'extraBinPaths'
| 'extraEnv'
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
| 'strictSsl'
| 'unsafePerm'
| 'userAgent'
| 'verifyStoreIntegrity'
>> &
Partial<Pick<ConfigContext,
| 'selectedProjectsGraph'
>> & {
  access?: 'public' | 'restricted'
  argv: {
    original: string[]
  }
  reportSummary?: boolean
} & PublishPackedPkgOptions

export type RecursivePublishedPackage = PublishSummary | { name?: string, version?: string }

export async function recursivePublish (
  opts: PublishRecursiveOpts & Required<Pick<ConfigContext, 'selectedProjectsGraph'>>
): Promise<{ exitCode: number, publishedPackages: RecursivePublishedPackage[] }> {
  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
  const { resolve } = createResolver({
    ...opts,
    configByUri: opts.configByUri,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
  const pkgsToPublish = await pFilter(pkgs, async (pkg) => {
    if (!pkg.manifest.name || !pkg.manifest.version || pkg.manifest.private) return false
    if (opts.force) return true
    return !(await isAlreadyPublished({
      dir: pkg.rootDir,
      lockfileDir: opts.lockfileDir ?? pkg.rootDir,
      registries: opts.registries,
      resolve,
    }, pkg.manifest.name, pkg.manifest.version))
  })
  const publishedPkgDirs = new Set<ProjectRootDir>(pkgsToPublish.map(({ rootDir }) => rootDir))
  const publishedPackages: RecursivePublishedPackage[] = []
  if (publishedPkgDirs.size === 0) {
    logger.info({
      message: 'There are no new packages that should be published',
      prefix: opts.dir,
    })
  } else {
    const appendedArgs: string[] = []
    if (opts.cliOptions['access']) {
      appendedArgs.push(`--access=${opts.cliOptions['access'] as string}`)
    }
    if (opts.dryRun) {
      appendedArgs.push('--dry-run')
    }
    if (opts.force) {
      appendedArgs.push('--force')
    }
    if (opts.cliOptions['otp']) {
      appendedArgs.push(`--otp=${opts.cliOptions['otp'] as string}`)
    }
    const chunks = sortProjects(opts.selectedProjectsGraph)
    const tag = opts.tag ?? 'latest'
    for (const chunk of chunks) {
      // We can't run publish concurrently due to the npm CLI asking for OTP.
      // NOTE: If we solve the OTP issue, we still need to limit packages concurrency.
      // Otherwise, publishing will consume too much resources.
      // See related issue: https://github.com/pnpm/pnpm/issues/6968
      for (const pkgDir of chunk) {
        if (!publishedPkgDirs.has(pkgDir)) continue
        const pkg = opts.selectedProjectsGraph[pkgDir].package
        const registry = pkg.manifest.publishConfig?.registry ?? pickRegistryForPackage(opts.registries, pkg.manifest.name!)
        // eslint-disable-next-line no-await-in-loop
        const publishResult = await publish({
          ...opts,
          dir: pkg.rootDir,
          argv: {
            original: [
              'publish',
              '--tag',
              tag,
              '--registry',
              registry,
              ...appendedArgs,
            ],
          },
          gitChecks: false,
          recursive: false,
        }, [pkg.rootDir])
        if (publishResult?.publishSummary != null) {
          publishedPackages.push(publishResult.publishSummary)
        } else {
          // Fallback for paths that don't produce a full PublishSummary (e.g. dry run via the
          // legacy npm-CLI bridge, or future call sites that bypass publishPackedPkg).
          const publishedManifest = publishResult?.publishedManifest ?? publishResult?.manifest
          if (publishedManifest != null) {
            publishedPackages.push(pick(['name', 'version'], publishedManifest))
          } else if (publishResult?.exitCode) {
            return { exitCode: publishResult.exitCode, publishedPackages }
          }
        }
      }
    }
  }
  if (opts.reportSummary) {
    await writeJsonFile(path.join(opts.lockfileDir ?? opts.dir, 'pnpm-publish-summary.json'), { publishedPackages })
  }
  return { exitCode: 0, publishedPackages }
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
): Promise<boolean> {
  try {
    await opts.resolve({ alias: pkgName, bareSpecifier: pkgVersion }, {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.dir,
    })
    return true
  } catch (err: any) { // eslint-disable-line
    return false
  }
}
