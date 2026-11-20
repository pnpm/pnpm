import path from 'node:path'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { createResolver } from '@pnpm/installing.client'
import { logger } from '@pnpm/logger'
import type { ResolveFunction } from '@pnpm/resolving.resolver-base'
import type { Project, ProjectRootDir, RegistriesByScope } from '@pnpm/types'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'
import pFilter from 'p-filter'
import { pick } from 'ramda'
import { writeJsonFile } from 'write-json-file'

import { publishedName } from '../publishedNames.js'
import { batchPublishPackages } from './batchPublish.js'
import { publish, type PublishResult } from './publish.js'
import { getPublishConfigRegistry, type PublishPackedPkgOptions, type PublishSummary } from './publishPackedPkg.js'
import { parseSupportedRegistryUrl } from './registryConfigKeys.js'

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
| 'allProjects'
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

type RecursivePublishOpts = PublishRecursiveOpts & Required<Pick<ConfigContext, 'selectedProjectsGraph'>>

export async function recursivePublish (
  opts: RecursivePublishOpts
): Promise<{ exitCode: number, publishedPackages: RecursivePublishedPackage[] }> {
  const pkgsToPublish = await selectUnpublishedPackages(opts)
  const publishedPkgDirs = new Set<ProjectRootDir>(pkgsToPublish.map(({ rootDir }) => rootDir))
  const publishedPackages: RecursivePublishedPackage[] = []
  if (publishedPkgDirs.size === 0) {
    logger.info({
      message: 'There are no new packages that should be published',
      prefix: opts.dir,
    })
  } else {
    const exitCode = await publishSelectedPackages(opts, { publishedPkgDirs, publishedPackages })
    if (exitCode !== 0) return { exitCode, publishedPackages }
  }
  if (opts.reportSummary) {
    await writeJsonFile(path.join(opts.lockfileDir ?? opts.dir, 'pnpm-publish-summary.json'), { publishedPackages })
  }
  return { exitCode: 0, publishedPackages }
}

async function selectUnpublishedPackages (opts: RecursivePublishOpts): Promise<Project[]> {
  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
  const getResolver = createResolverCache(opts)
  return pFilter(pkgs, async (pkg) => {
    if (!pkg.manifest.name || !pkg.manifest.version || pkg.manifest.private) return false
    if (opts.force) return true
    const targetName = publishedName(pkg.manifest)!
    const registriesByScope = routeToPublishConfigRegistry(opts.registriesByScope, targetName, getPublishConfigRegistry(pkg.manifest.publishConfig, targetName))
    return !(await isAlreadyPublished({
      dir: pkg.rootDir,
      lockfileDir: opts.lockfileDir ?? pkg.rootDir,
      resolve: getResolver(registriesByScope),
    }, targetName, pkg.manifest.version))
  })
}

function createResolverCache (opts: RecursivePublishOpts): (registriesByScope: RegistriesByScope) => ResolveFunction {
  const resolverByRegistries = new Map<string, ResolveFunction>()
  return (registriesByScope) => {
    const key = JSON.stringify(registriesByScope)
    let resolve = resolverByRegistries.get(key)
    if (resolve == null) {
      resolve = createResolver({
        ...opts,
        configByUri: opts.configByUri,
        registriesByScope,
        retry: {
          factor: opts.fetchRetryFactor,
          maxTimeout: opts.fetchRetryMaxtimeout,
          minTimeout: opts.fetchRetryMintimeout,
          retries: opts.fetchRetries,
        },
        timeout: opts.fetchTimeout,
      }).resolve
      resolverByRegistries.set(key, resolve)
    }
    return resolve
  }
}

interface PublishTargets {
  publishedPkgDirs: Set<ProjectRootDir>
  publishedPackages: RecursivePublishedPackage[]
}

/** Publishes the packages in `publishedPkgDirs`, appending each summary to `publishedPackages`. Returns the exit code. */
async function publishSelectedPackages (opts: RecursivePublishOpts, targets: PublishTargets): Promise<number> {
  const appendedArgs = forwardedPublishArgs(opts)
  const projectDependencies = filteredProjectsDependencies(opts)
  const tag = opts.tag ?? 'latest'
  if (!opts.batch) {
    return publishEachPackage(opts, { ...targets, appendedArgs, projectDependencies, tag })
  }
  const sortedPkgs = graphSequencer(projectDependencies).order
    .filter((pkgDir) => targets.publishedPkgDirs.has(pkgDir))
    .map((pkgDir) => opts.selectedProjectsGraph[pkgDir].package)
  targets.publishedPackages.push(...await batchPublishPackages(sortedPkgs, { ...opts, tag }))
  return 0
}

function forwardedPublishArgs (opts: RecursivePublishOpts): string[] {
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
  return appendedArgs
}

interface PublishEachPackageOptions extends PublishTargets {
  appendedArgs: string[]
  projectDependencies: Map<ProjectRootDir, ProjectRootDir[]>
  tag: string
}

async function publishEachPackage (opts: RecursivePublishOpts, publishOpts: PublishEachPackageOptions): Promise<number> {
  const { publishedPkgDirs, publishedPackages, projectDependencies } = publishOpts
  let firstError: unknown
  let exitCode = 0
  await scheduleGraph(projectDependencies, {
    bail: true,
    concurrency: 1,
    runNode: async (pkgDir): Promise<TaskCompletion> => {
      try {
        if (!publishedPkgDirs.has(pkgDir)) return 'passed'
        const publishResult = await publishPackage(opts, opts.selectedProjectsGraph[pkgDir].package, publishOpts)
        const failureExitCode = recordPublishResult(publishResult, publishedPackages)
        if (failureExitCode == null) return 'passed'
        exitCode = failureExitCode
        return 'aborted'
      } catch (error: unknown) {
        firstError ??= error
        return 'aborted'
      }
    },
    onNodeSkipped: () => {},
  })
  if (firstError != null) throw firstError
  return exitCode
}

async function publishPackage (
  opts: RecursivePublishOpts,
  pkg: Project,
  { appendedArgs, tag }: Pick<PublishEachPackageOptions, 'appendedArgs' | 'tag'>
): Promise<PublishResult> {
  const targetName = publishedName(pkg.manifest)!
  const registry = getPublishConfigRegistry(pkg.manifest.publishConfig, targetName) ?? pickRegistryForPackage(opts.registriesByScope, targetName)
  const commandArgs = opts.stage ? ['stage', 'publish'] : ['publish']
  return publish({
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
}

/**
 * Appends what `publishResult` published to `publishedPackages`. Returns the
 * exit code of a publish that failed without publishing anything.
 */
function recordPublishResult (
  publishResult: PublishResult | undefined,
  publishedPackages: RecursivePublishedPackage[]
): number | undefined {
  if (publishResult?.publishSummary != null) {
    publishedPackages.push(publishResult.publishSummary)
    return undefined
  }
  // Fallback for paths that don't produce a full PublishSummary (e.g. dry run via the
  // legacy npm-CLI bridge, or future call sites that bypass publishPackedPkg).
  const publishedManifest = publishResult?.publishedManifest ?? publishResult?.manifest
  if (publishedManifest != null) {
    publishedPackages.push(pick(['name', 'version'], publishedManifest))
    return undefined
  }
  return publishResult?.exitCode ? publishResult.exitCode : undefined
}

/**
 * Routes `pkgName` to its `publishConfig` registry, so the already-published
 * probe reads the registry the publish writes to.
 */
function routeToPublishConfigRegistry (
  registriesByScope: RegistriesByScope,
  pkgName: string,
  publishConfigRegistry: string | undefined
): RegistriesByScope {
  if (publishConfigRegistry == null) return registriesByScope
  const scope = pkgName.startsWith('@') ? pkgName.slice(0, pkgName.indexOf('/')) : 'default'
  return { ...registriesByScope, [scope]: parseSupportedRegistryUrl(publishConfigRegistry)?.normalizedUrl ?? publishConfigRegistry }
}

async function isAlreadyPublished (
  opts: {
    dir: string
    lockfileDir: string
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
