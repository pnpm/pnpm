import {
  type ClientOptions,
  createResolver,
  type ResolveFunction,
} from '@pnpm/client'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { type PackageVersionPolicy, type DependencyManifest } from '@pnpm/types'

interface GetManifestOpts {
  dir: string
  lockfileDir: string
  rawConfig: object
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
}

export type ManifestGetterOptions = Omit<ClientOptions, 'authConfig' | 'minimumReleaseAgeExclude'>
& GetManifestOpts
& { fullMetadata: boolean, rawConfig: Record<string, string> }

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, bareSpecifier: string) => Promise<DependencyManifest | null> {
  const publishedByExclude = opts.minimumReleaseAgeExclude
    ? createPackageVersionPolicy(opts.minimumReleaseAgeExclude)
    : undefined

  const { resolve } = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
    filterMetadata: false, // We need all the data from metadata for "outdated --long" to work.
    strictPublishedByCheck: Boolean(opts.minimumReleaseAge),
  })

  const publishedBy = opts.minimumReleaseAge
    ? new Date(Date.now() - opts.minimumReleaseAge * 60 * 1000)
    : undefined

  return getManifest.bind(null, {
    ...opts,
    resolve,
    publishedBy,
    publishedByExclude,
  })
}

export async function getManifest (
  opts: GetManifestOpts & {
    resolve: ResolveFunction
    publishedBy?: Date
    publishedByExclude?: PackageVersionPolicy
  },
  packageName: string,
  bareSpecifier: string
): Promise<DependencyManifest | null> {
  try {
    const resolution = await opts.resolve({ alias: packageName, bareSpecifier }, {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.dir,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
    })
    return resolution?.manifest ?? null
  } catch (err) {
    if ((err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION' && opts.publishedBy) {
      // No versions found that meet the minimumReleaseAge requirement
      return null
    }
    throw err
  }
}
