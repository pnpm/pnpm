import {
  type ClientOptions,
  createResolver,
  type ResolveFunction,
} from '@pnpm/client'
import { createMatcher } from '@pnpm/matcher'
import { type DependencyManifest } from '@pnpm/types'

interface GetManifestOpts {
  dir: string
  lockfileDir: string
  rawConfig: object
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
}

export type ManifestGetterOptions = Omit<ClientOptions, 'authConfig'>
& GetManifestOpts
& { fullMetadata: boolean, rawConfig: Record<string, string> }

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, bareSpecifier: string) => Promise<DependencyManifest | null> {
  const { resolve } = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
    filterMetadata: Boolean(opts.minimumReleaseAge),
    strictPublishedByCheck: Boolean(opts.minimumReleaseAge),
  })

  const publishedBy = opts.minimumReleaseAge
    ? new Date(Date.now() - opts.minimumReleaseAge * 60 * 1000)
    : undefined

  const isExcludedMatcher = opts.minimumReleaseAgeExclude
    ? createMatcher(opts.minimumReleaseAgeExclude)
    : undefined

  return (packageName: string, bareSpecifier: string) =>
    getManifest(resolve, opts, packageName, bareSpecifier, publishedBy, isExcludedMatcher)
}

export async function getManifest (
  resolve: ResolveFunction,
  opts: GetManifestOpts,
  packageName: string,
  bareSpecifier: string,
  publishedBy?: Date,
  isExcludedMatcher?: ((packageName: string) => boolean)
): Promise<DependencyManifest | null> {
  const isExcluded = isExcludedMatcher?.(packageName)
  const effectivePublishedBy = isExcluded ? undefined : publishedBy

  try {
    const resolution = await resolve({ alias: packageName, bareSpecifier }, {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.dir,
      publishedBy: effectivePublishedBy,
    })
    return resolution?.manifest ?? null
  } catch (err) {
    if ((err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION' && effectivePublishedBy) {
      // No versions found that meet the minimumReleaseAge requirement
      return null
    }
    throw err
  }
}
