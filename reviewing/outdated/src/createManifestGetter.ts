import {
  type ClientOptions,
  createResolver,
  type ResolveFunction,
} from '@pnpm/client'
import { createVersionMatcher } from '@pnpm/matcher'
import { type DependencyManifest } from '@pnpm/types'

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
  const isExcludedMatcher = opts.minimumReleaseAgeExclude
    ? createVersionMatcher(opts.minimumReleaseAgeExclude)
    : undefined

  const { resolve } = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
    filterMetadata: false, // We need all the data from metadata for "outdated --long" to work.
    strictPublishedByCheck: Boolean(opts.minimumReleaseAge),
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
  })

  const publishedBy = opts.minimumReleaseAge
    ? new Date(Date.now() - opts.minimumReleaseAge * 60 * 1000)
    : undefined

  return getManifest.bind(null, {
    ...opts,
    resolve,
    publishedBy,
    isExcludedMatcher,
  })
}

export async function getManifest (
  opts: GetManifestOpts & {
    resolve: ResolveFunction
    publishedBy?: Date
    isExcludedMatcher?: ((packageName: string, version?: string) => boolean)
  },
  packageName: string,
  bareSpecifier: string
): Promise<DependencyManifest | null> {
  const isExcludedByNameOnly = opts.isExcludedMatcher?.(packageName)
  const effectivePublishedBy = isExcludedByNameOnly ? undefined : opts.publishedBy

  try {
    const resolution = await opts.resolve({ alias: packageName, bareSpecifier }, {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.dir,
      publishedBy: effectivePublishedBy,
    })
    return resolution?.manifest ?? null
  } catch (err) {
    if ((err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION' && effectivePublishedBy) {
      return null
    }
    throw err
  }
}
