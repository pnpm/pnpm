import { getPublishedByPolicy } from '@pnpm/config.version-policy'
import {
  type ClientOptions,
  createResolver,
  type ResolveFunction,
} from '@pnpm/installing.client'
import type { DependencyManifest, PackageVersionPolicy, RegistryConfig } from '@pnpm/types'

interface GetManifestOpts {
  dir: string
  lockfileDir: string
  configByUri: object
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  minimumReleaseAgeIgnoreMissingTime?: boolean
  minimumReleaseAgeStrict?: boolean
}

export type ManifestGetterOptions = Omit<ClientOptions, 'configByUri' | 'minimumReleaseAgeExclude' | 'storeIndex'>
& GetManifestOpts
& { fullMetadata: boolean, configByUri: Record<string, RegistryConfig> }

export function createManifestGetter (
  opts: ManifestGetterOptions
): (packageName: string, bareSpecifier: string) => Promise<DependencyManifest | null> {
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)

  const { resolve } = createResolver({
    ...opts,
    configByUri: opts.configByUri,
    filterMetadata: false, // We need all the data from metadata for "outdated --long" to work.
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime,
  })

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
    // No mature version found within range: the resolver fell back to the
    // lowest immature pick and flagged it inline. `outdated` shouldn't
    // present an immature version as "available", so treat it as no match
    // — matching the pre-violation-collection behavior when the resolver
    // threw `NO_MATURE_MATCHING_VERSION`.
    if (resolution?.policyViolation?.code === 'MINIMUM_RELEASE_AGE_VIOLATION') {
      return null
    }
    return resolution?.manifest ?? null
  } catch (err) {
    const code = (err as { code?: string }).code
    if (opts.publishedBy && code === 'ERR_PNPM_NO_MATCHING_VERSION') {
      // No version satisfies the range at all (not a maturity issue).
      // Pre-violation-collection this branch also covered the maturity
      // case via `NO_MATURE_MATCHING_VERSION`; with always-defer, that
      // case is handled above as a `policyViolation`.
      return null
    }
    throw err
  }
}
