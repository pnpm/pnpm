import path from 'node:path'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { createResolver } from '@pnpm/installing.client'
import { logger } from '@pnpm/logger'
import { getCurrentBranch } from '@pnpm/network.git-utils'
import { assembleReleasePlan, readChangeIntents, readLedger, toProjectDir } from '@pnpm/releasing.versioning'
import type { ResolveFunction } from '@pnpm/resolving.resolver-base'
import type { ProjectRootDir, Registries } from '@pnpm/types'
import { sortFilteredProjects } from '@pnpm/workspace.projects-sorter'
import pFilter from 'p-filter'
import { pick } from 'ramda'
import { writeJsonFile } from 'write-json-file'

import { batchPublishPackages } from './batchPublish.js'
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
| 'versioning'
| 'filter'
>> &
Partial<Pick<ConfigContext,
| 'selectedProjectsGraph'
| 'allProjectsGraph'
| 'prodAllProjectsGraph'
| 'prodOnlySelectedProjectDirs'
| 'allProjects'
>> & {
  access?: 'public' | 'restricted'
  argv: {
    original: string[]
  }
  batch?: boolean
  reportSummary?: boolean
  snapshot?: string | boolean
  workspaceVersions?: Readonly<Record<string, string>>
} & PublishPackedPkgOptions

export type RecursivePublishedPackage = PublishSummary | { name?: string, version?: string }

export async function recursivePublish (
  opts: PublishRecursiveOpts & Required<Pick<ConfigContext, 'selectedProjectsGraph'>>
): Promise<{ exitCode: number, publishedPackages: RecursivePublishedPackage[] }> {
  let pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
  let workspaceVersions = opts.workspaceVersions
  let snapshotTag: string | undefined
  if (opts.snapshot) {
    const snapshot = await createSnapshotPlan(opts)
    workspaceVersions = snapshot.workspaceVersions
    snapshotTag = snapshot.tag
    const plannedDirs = new Set(snapshot.projectDirs)
    pkgs = pkgs.filter((pkg) => plannedDirs.has(pkg.rootDir))
  }
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
    if (workspaceVersions != null) return true
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
    const chunks = sortFilteredProjects(opts)
    const tag = snapshotTag ?? opts.tag ?? 'latest'
    if (opts.batch) {
      const sortedPkgs = chunks
        .flat()
        .filter((pkgDir) => publishedPkgDirs.has(pkgDir))
        .map((pkgDir) => opts.selectedProjectsGraph[pkgDir].package)
      publishedPackages.push(...await batchPublishPackages(sortedPkgs, { ...opts, tag, workspaceVersions, lane: snapshotTag ?? opts.lane }))
    } else {
      const commandArgs = opts.stage ? ['stage', 'publish'] : ['publish']
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
            snapshot: false,
            workspaceVersions,
            lane: snapshotTag ?? opts.lane,
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
  }
  if (opts.reportSummary) {
    await writeJsonFile(path.join(opts.lockfileDir ?? opts.dir, 'pnpm-publish-summary.json'), { publishedPackages })
  }
  return { exitCode: 0, publishedPackages }
}

async function createSnapshotPlan (
  opts: PublishRecursiveOpts & Required<Pick<ConfigContext, 'selectedProjectsGraph'>>
): Promise<{ projectDirs: string[], tag: string, workspaceVersions: Record<string, string> }> {
  const projects = opts.allProjects ?? []
  if (projects.length === 0) {
    throw new Error('Cannot assemble a snapshot release plan without workspace projects')
  }
  const requestedTag = typeof opts.snapshot === 'string'
    ? opts.snapshot
    : await getCurrentBranch() ?? 'snapshot'
  const tag = normalizeSnapshotTag(requestedTag)
  const plan = assembleReleasePlan({
    workspaceDir: opts.workspaceDir,
    projects: projects.map(({ rootDir, manifest }) => ({ rootDir, manifest })),
    intents: await readChangeIntents(opts.workspaceDir),
    ledger: await readLedger(opts.workspaceDir),
    versioning: opts.versioning,
    filter: (opts.filter ?? []).length > 0
      ? new Set(Object.keys(opts.selectedProjectsGraph).map((rootDir) => toProjectDir(opts.workspaceDir, rootDir)))
      : undefined,
    snapshotSuffix: createSnapshotSuffix(tag),
    enforceWorkspaceProtocol: true,
  })
  return {
    projectDirs: plan.releases.map((release) => release.rootDir),
    tag,
    workspaceVersions: Object.fromEntries(plan.releases.map((release) => [release.name, release.newVersion])),
  }
}

export function createSnapshotSuffix (tag: string, now = new Date()): string {
  return `${tag}-${now.toISOString().replace(/[-:TZ.]/g, '').slice(0, 14)}`
}

function normalizeSnapshotTag (tag: string): string {
  const normalized = tag
    .replace(/[^0-9a-z-]+/gi, '-')
    .replace(/^-+|-+$/g, '')
  if (normalized.length === 0) {
    throw new Error(`Cannot derive a snapshot tag from "${tag}"`)
  }
  return normalized
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
