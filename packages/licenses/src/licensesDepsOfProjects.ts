import path from 'path'
import {
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import { createMatcher } from '@pnpm/matcher'
import { readModulesManifest } from '@pnpm/modules-yaml'
import {
  IncludedDependencies,
  ProjectManifest,
  Registries,
} from '@pnpm/types'
import unnest from 'ramda/src/unnest'
import { licences, LicensePackage } from './licenses'
import { ClientOptions } from '@pnpm/client'

interface GetManifestOpts {
  dir: string
  lockfileDir: string
  virtualStoreDir: string
  rawConfig: object
  registries: Registries
}

export type ManifestGetterOptions = Omit<ClientOptions, 'authConfig'>
& GetManifestOpts
& { fullMetadata: boolean, rawConfig: Record<string, string> }

export async function licensesDepsOfProjects (
  pkgs: Array<{ dir: string, manifest: ProjectManifest }>,
  args: string[],
  opts: Omit<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir' | 'virtualStoreDir'> & {
    compatible?: boolean
    ignoreDependencies?: Set<string>
    include: IncludedDependencies
  } & Partial<Pick<ManifestGetterOptions, 'fullMetadata' | 'lockfileDir' | 'virtualStoreDir'>>
): Promise<LicensePackage[][]> {
  if (!opts.lockfileDir) {
    return unnest(await Promise.all(
      pkgs.map(async (pkg) => {
        return licensesDepsOfProjects([pkg], args, { ...opts, lockfileDir: pkg.dir })
      }
      )
    ))
  }

  const lockfileDir = opts.lockfileDir ?? opts.dir
  const modules = await readModulesManifest(path.join(lockfileDir, 'node_modules'))
  const virtualStoreDir = modules?.virtualStoreDir ?? path.join(lockfileDir, 'node_modules/.pnpm')
  const currentLockfile = await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }) ?? currentLockfile
  return Promise.all(pkgs.map(async ({ dir, manifest }) => {
    const match = (args.length > 0) && createMatcher(args) || undefined
    return licences({
      compatible: opts.compatible,
      currentLockfile,
      ignoreDependencies: opts.ignoreDependencies,
      include: opts.include,
      lockfileDir,
      manifest,
      match,
      prefix: dir,
      registries: opts.registries,
      wantedLockfile,
    })
  }))
}
