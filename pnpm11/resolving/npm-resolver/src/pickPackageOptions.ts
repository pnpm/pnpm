import type { PickPackageOptions } from './pickPackage.js'
import { preferredVersionSelectorsFor } from './preferredVersionSelectors.js'
import type { RegistrySpecRequest } from './resolverTypes.js'

export function createPickPackageOptions (
  { ctx, wantedDependency, opts, spec, registry }: RegistrySpecRequest
): PickPackageOptions {
  const authHeaderValue = ctx.getAuthHeaderValueByURI(registry, { pkgName: spec.name })
  return {
    pickLowestVersion: opts.pickLowestVersion,
    publishedBy: opts.publishedBy,
    fallbackPublishedBy: opts.fallbackPublishedBy,
    publishedByExclude: opts.publishedByExclude,
    authHeaderValue,
    dryRun: opts.dryRun === true,
    preferredVersionSelectors: preferredVersionSelectorsFor(opts, spec.name),
    registry,
    includeLatestTag: opts.update === 'latest',
    updateChecksums: opts.updateChecksums || opts.updatePatches,
    refreshMetadata: opts.update === 'compatible' || opts.update === 'latest' || opts.updateRequested === true,
    optional: wantedDependency.optional,
    trustPolicy: opts.trustPolicy,
  }
}
