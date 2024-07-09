import path from 'path'
import {
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import { createMatcher } from '@pnpm/matcher'
import { readModulesManifest } from '@pnpm/modules-yaml'
import {
  type IncludedDependencies,
  type ProjectManifest,
  type ProjectRootDir,
} from '@pnpm/types'
import unnest from 'ramda/src/unnest'
import { createManifestGetter, type ManifestGetterOptions } from './createManifestGetter'
import { outdated, type OutdatedPackage } from './outdated'

export async function outdatedDepsOfProjects (
  pkgs: Array<{ rootDir: ProjectRootDir, manifest: ProjectManifest }>,
  args: string[],
  opts: Omit<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir'> & {
    compatible?: boolean
    ignoreDependencies?: string[]
    include: IncludedDependencies
  } & Partial<Pick<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir'>>
): Promise<OutdatedPackage[][]> {
  if (!opts.lockfileDir) {
    return unnest(await Promise.all(
      pkgs.map(async (pkg) =>
        outdatedDepsOfProjects([pkg], args, { ...opts, lockfileDir: pkg.rootDir })
      )
    ))
  }
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const modules = await readModulesManifest(path.join(lockfileDir, 'node_modules'))
  const virtualStoreDir = modules?.virtualStoreDir ?? path.join(lockfileDir, 'node_modules/.pnpm')
  const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }) ?? currentLockfile
  const getLatestManifest = createManifestGetter({
    ...opts,
    fullMetadata: opts.fullMetadata === true,
    lockfileDir,
  })
  return Promise.all(pkgs.map(async ({ rootDir, manifest }): Promise<OutdatedPackage[]> => {
    const match = (args.length > 0) && createMatcher(args) || undefined
    return outdated({
      compatible: opts.compatible,
      currentLockfile,
      getLatestManifest,
      ignoreDependencies: opts.ignoreDependencies,
      include: opts.include,
      lockfileDir,
      manifest,
      match,
      prefix: rootDir,
      registries: opts.registries,
      wantedLockfile,
    })
  }))
}
