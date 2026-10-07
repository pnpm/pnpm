import { getPublishedByPolicy } from '@pnpm/config.version-policy'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import { createNpmResolver, type ResolverFactoryOptions } from '@pnpm/resolving.npm-resolver'
import type { RegistryConfig } from '@pnpm/types'

export type ResolveConfigDep = ReturnType<typeof createNpmResolver>['resolveFromNpm']

export type ConfigDepResolverOpts = CreateFetchFromRegistryOptions & ResolverFactoryOptions & {
  configByUri?: Record<string, RegistryConfig>
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  minimumReleaseAgeIgnoreMissingTime?: boolean
}

/**
 * Resolves config dependencies under the `minimumReleaseAge` cutoff that the
 * config dependency verifier enforces, so a resolution never records a
 * version that the next clean install rejects.
 */
export function createConfigDepResolver (opts: ConfigDepResolverOpts): ResolveConfigDep {
  const fetch = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {})
  const { resolveFromNpm } = createNpmResolver(fetch, getAuthHeader, {
    ...opts,
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime ?? opts.ignoreMissingTimeField,
  })
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
  return async (wantedDependency, resolveOpts) => {
    const result = await resolveFromNpm(wantedDependency, { ...resolveOpts, publishedBy, publishedByExclude })
    const violation = result?.policyViolation
    if (violation != null) {
      throw new PnpmError('BAD_CONFIG_DEP', `Configuration dependency "${violation.name}@${violation.version}" ${violation.reason}`)
    }
    return result
  }
}
