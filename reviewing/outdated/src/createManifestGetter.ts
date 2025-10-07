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
  registrySupportsTimeField?: boolean
}

export type ManifestGetterOptions = Omit<ClientOptions, 'authConfig'>
& GetManifestOpts
& { fullMetadata: boolean, rawConfig: Record<string, string> }

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, bareSpecifier: string) => Promise<DependencyManifest | null> {
  const fullMetadata = Boolean(opts.minimumReleaseAge) && !opts.registrySupportsTimeField
  const { resolve } = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
    filterMetadata: fullMetadata,
    fullMetadata,
    strictPublishedByCheck: Boolean(opts.minimumReleaseAge),
  })

  const publishedBy = opts.minimumReleaseAge
    ? new Date(Date.now() - opts.minimumReleaseAge * 60 * 1000)
    : undefined

  const isExcludedMatcher = opts.minimumReleaseAgeExclude
    ? createMatcher(opts.minimumReleaseAgeExclude)
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
    isExcludedMatcher?: ((packageName: string) => boolean)
  },
  packageName: string,
  bareSpecifier: string
): Promise<DependencyManifest | null> {
  const isExcluded = opts.isExcludedMatcher?.(packageName)
  const effectivePublishedBy = isExcluded ? undefined : opts.publishedBy

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
      // No versions found that meet the minimumReleaseAge requirement
      return null
    }
    throw err
  }
}
