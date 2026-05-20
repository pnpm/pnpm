import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { createMatcher } from '@pnpm/config.matcher'
import { getPublishedByPolicy } from '@pnpm/config.version-policy'
import { type ClientOptions, createResolver } from '@pnpm/installing.client'
import {
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile.fs'
import type {
  IncludedDependencies,
  ProjectManifest,
  ProjectRootDir,
  RegistryConfig,
} from '@pnpm/types'
import { unnest } from 'ramda'

import { outdated, type OutdatedPackage } from './outdated.js'

export type OutdatedDepsOfProjectsOptions = Omit<ClientOptions, 'configByUri' | 'minimumReleaseAgeExclude' | 'storeIndex'>
& {
  dir: string
  lockfileDir?: string
  configByUri: Record<string, RegistryConfig>
  fullMetadata?: boolean
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  minimumReleaseAgeIgnoreMissingTime?: boolean
  minimumReleaseAgeStrict?: boolean
}

export async function outdatedDepsOfProjects (
  pkgs: Array<{ rootDir: ProjectRootDir, manifest: ProjectManifest }>,
  args: string[],
  opts: OutdatedDepsOfProjectsOptions & {
    catalogs?: Catalogs
    compatible?: boolean
    ignoreDependencies?: string[]
    include: IncludedDependencies
  }
): Promise<OutdatedPackage[][]> {
  if (!opts.lockfileDir) {
    return unnest(await Promise.all(
      pkgs.map(async (pkg) =>
        outdatedDepsOfProjects([pkg], args, { ...opts, lockfileDir: pkg.rootDir })
      )
    ))
  }
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const internalPnpmDir = path.join(path.join(lockfileDir, 'node_modules/.pnpm'))
  const currentLockfile = await readCurrentLockfile(internalPnpmDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }) ?? currentLockfile
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)

  const { resolveLatest } = createResolver({
    ...opts,
    configByUri: opts.configByUri,
    filterMetadata: false,
    fullMetadata: opts.fullMetadata === true || Boolean(opts.minimumReleaseAge),
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime,
  })

  return Promise.all(pkgs.map(async ({ rootDir, manifest }): Promise<OutdatedPackage[]> => {
    const match = (args.length > 0) && createMatcher(args) || undefined
    return outdated({
      catalogs: opts.catalogs,
      compatible: opts.compatible,
      currentLockfile,
      resolveLatest,
      ignoreDependencies: opts.ignoreDependencies,
      include: opts.include,
      lockfileDir,
      manifest,
      match,
      minimumReleaseAge: opts.minimumReleaseAge,
      minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
      prefix: rootDir,
      publishedBy,
      publishedByExclude,
      wantedLockfile,
    })
  }))
}
