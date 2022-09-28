import path from 'path'
import {
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import matcher from '@pnpm/matcher'
import { read as readModulesManifest } from '@pnpm/modules-yaml'
import {
  IncludedDependencies,
  ProjectManifest,
} from '@pnpm/types'
import unnest from 'ramda/src/unnest'
import { createManifestGetter, ManifestGetterOptions } from './createManifestGetter'
import outdated, { OutdatedPackage } from './outdated'

export default async function outdatedDepsOfProjects (
  pkgs: Array<{dir: string, manifest: ProjectManifest}>,
  args: string[],
  opts: Omit<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir'> & {
    compatible?: boolean
    ignoreDependencies?: Set<string>
    include: IncludedDependencies
  } & Partial<Pick<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir'>>
): Promise<OutdatedPackage[][]> {
  if (!opts.lockfileDir) {
    return unnest(await Promise.all(
      pkgs.map(async (pkg) =>
        outdatedDepsOfProjects([pkg], args, { ...opts, lockfileDir: pkg.dir })
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
  return Promise.all(pkgs.map(async ({ dir, manifest }) => {
    const match = (args.length > 0) && matcher(args) || undefined
    return outdated({
      compatible: opts.compatible,
      currentLockfile,
      getLatestManifest,
      ignoreDependencies: opts.ignoreDependencies,
      include: opts.include,
      lockfileDir,
      manifest,
      match,
      prefix: dir,
      wantedLockfile,
    })
  }))
}
