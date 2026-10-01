import type { LatestInfo, LatestQuery, ResolveOptions } from '@pnpm/resolving.resolver-base'

import {
  parseBareSpecifier,
  parseJsrSpecifierToRegistryPackageSpec,
  parseNamedRegistrySpecifierToRegistryPackageSpec,
} from './parseBareSpecifier.js'
import type { NpmResolver, ResolveLatestFromNpmStyle } from './resolverTypes.js'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE } from './violationCodes.js'

export function isNpmSpec (query: LatestQuery, defaultRegistry: string): boolean {
  const { alias, bareSpecifier } = query.wantedDependency
  if (!bareSpecifier) return alias != null
  return parseBareSpecifier(bareSpecifier, alias, 'latest', defaultRegistry) != null
}

export function isJsrSpec (query: LatestQuery): boolean {
  if (!query.wantedDependency.bareSpecifier?.startsWith('jsr:')) return false
  return parseJsrSpecifierToRegistryPackageSpec(
    query.wantedDependency.bareSpecifier,
    query.wantedDependency.alias,
    'latest'
  ) != null
}

export function isNamedRegistrySpec (
  query: LatestQuery,
  knownRegistryNames: ReadonlySet<string>
): boolean {
  if (!query.wantedDependency.bareSpecifier) return false
  try {
    return parseNamedRegistrySpecifierToRegistryPackageSpec(
      query.wantedDependency.bareSpecifier,
      knownRegistryNames,
      query.wantedDependency.alias,
      'latest'
    ) != null
  } catch {
    return false
  }
}

export function createResolveLatest (
  resolve: NpmResolver,
  matches: (query: LatestQuery) => boolean
): ResolveLatestFromNpmStyle {
  return async (query: LatestQuery, opts: ResolveOptions): Promise<LatestInfo | undefined> => {
    if (!matches(query)) return undefined
    return resolveLatestInfo(resolve, query, opts)
  }
}

async function resolveLatestInfo (
  resolve: NpmResolver,
  query: LatestQuery,
  opts: ResolveOptions
): Promise<LatestInfo> {
  // Always pass the manifest's bareSpecifier so protocol-prefixed specs
  // (`jsr:@scope/pkg@^1.0.0`, `gh:owner/repo@^1.0.0`) still match their
  // resolver. In --compatible mode that range drives the pick; otherwise
  // `update: 'latest'` tells the resolver to ignore the range and take
  // the absolute newest.
  const bareSpecifier = query.wantedDependency.bareSpecifier ?? 'latest'
  const resolveOpts = query.compatible ? opts : { ...opts, update: 'latest' as const }
  try {
    const result = await resolve(
      { alias: query.wantedDependency.alias, bareSpecifier },
      resolveOpts
    )
    // Policy-blocked: handled but no latest to surface.
    if (result?.policyViolation?.code === MINIMUM_RELEASE_AGE_VIOLATION_CODE) {
      return {}
    }
    return { latestManifest: result?.manifest }
  } catch (err) {
    if (opts.publishedBy && (err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION') {
      return {}
    }
    throw err
  }
}
