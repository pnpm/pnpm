import path from 'node:path'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { createResolver } from '@pnpm/installing.client'
import { logger } from '@pnpm/logger'
import type { ResolveFunction } from '@pnpm/resolving.resolver-base'
import type { ProjectRootDir, RegistriesByScope } from '@pnpm/types'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'
import pFilter from 'p-filter'
import { pick } from 'ramda'
import { writeJsonFile } from 'write-json-file'

import { publishedName } from '../publishedNames.js'
import { batchPublishPackages } from './batchPublish.js'
import { publish } from './publish.js'
import type { PublishPackedPkgOptions, PublishSummary } from './publishPackedPkg.js'

export type PublishRecursiveOpts = Required<Pick<Config,
| 'bin'
| 'cacheDir'
| 'dir'
| 'pnpmHomeDir'
| 'configByUri'
| 'registriesByScope'
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
| 'offline'
| 'strictSsl'
| 'unsafePerm'
| 'userAgent'
| 'verifyStoreIntegrity'
| 'versioning'
>> &
Partial<Pick<ConfigContext,
| 'selectedProjectsGraph'
| 'allProjectsGraph'
| 'prodAllProjectsGraph'
| 'prodOnlySelectedProjectDirs'
>> & {
  access?: 'public' | 'restricted'
  argv: {
    original: string[]
  }
  batch?: boolean
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
      registriesByScope: opts.registriesByScope,
      resolve,
    }, publishedName(pkg.manifest)!, pkg.manifest.version))
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
    const projectDependencies = filteredProjectsDependencies(opts)
    const tag = opts.tag ?? 'latest'
    if (opts.batch) {
      const sortedPkgs = graphSequencer(projectDependencies).order
        .filter((pkgDir) => publishedPkgDirs.has(pkgDir))
        .map((pkgDir) => opts.selectedProjectsGraph[pkgDir].package)
      publishedPackages.push(...await batchPublishPackages(sortedPkgs, { ...opts, tag }))
    } else {
      const commandArgs = opts.stage ? ['stage', 'publish'] : ['publish']
      let firstError: unknown
      let exitCode = 0
      await scheduleGraph(projectDependencies, {
        bail: true,
        concurrency: 1,
        runNode: async (pkgDir): Promise<TaskCompletion> => {
          try {
            if (!publishedPkgDirs.has(pkgDir)) return 'passed'
            const pkg = opts.selectedProjectsGraph[pkgDir].package
            // The registry is picked by scope, so a `publishConfig.name` that
            // moves the package to another scope has to route by the new one.
            const registry = pkg.manifest.publishConfig?.registry ?? pickRegistryForPackage(opts.registriesByScope, publishedName(pkg.manifest)!)

            const publishResult = await publish({
              ...opts,
              dir: pkg.rootDir,
              argv: {
                original: [
                  ...commandArgs,
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
                exitCode = publishResult.exitCode
                return 'aborted'
              }
            }
            return 'passed'
          } catch (error: unknown) {
            firstError ??= error
            return 'aborted'
          }
        },
        onNodeSkipped: () => {},
      })
      if (firstError != null) throw firstError
      if (exitCode !== 0) return { exitCode, publishedPackages }
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
    registriesByScope: RegistriesByScope
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
